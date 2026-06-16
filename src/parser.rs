use crate::lexer::{TknSpan, Tkn, get_token_at};
use std::process::{Command, Stdio, ExitStatus};
use std::collections::VecDeque;
use std::iter::{Peekable};
use std::slice::Iter;
use std::process;
use std::fs::{File, OpenOptions};
use std::io::Write;
use anyhow::anyhow;
/* 
 * Recursive Descent Parser
 * See https://ruslanspivak.com/lsbasi-part7/ for an e.g.
 *
 * */
struct ChildPr { //a child process spawned by shell
    pub handle: Command,
    //I/O streams for redirection
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,

    pub prog_name: String,
    pub heredoc_content: Option<String>,
}

impl ChildPr {
    //for setting stdin/stdout for piping
    pub fn set_stdout(&mut self, fd: Stdio) {
        self.handle.stdout(fd);
    }
    pub fn set_stdin(&mut self, fd: Stdio) {
        self.handle.stdin(fd);
    }

    fn handle_redirect(&mut self) {
        if let Some(fd) = self.stdin.take() {
            self.handle.stdin(fd);
        }
        if let Some(fd) = self.stdout.take() {
            self.handle.stdout(fd);
        }
    }

    pub fn spawn(&mut self) -> anyhow::Result<process::Child> {
        self.handle_redirect(); //apply <, >, >> etc. 
        match self.handle.spawn() {
            Ok(mut c) => {
                if let Some(buf) = self.heredoc_content.take() {
                    if let Some(mut stdin) = c.stdin.take() {
                        let _ = stdin.write_all(buf.as_bytes());
                    }
                }
                Ok(c)
            }
            Err(e) => Err(anyhow!("{}", e)),
        }
    }

    //same as spawn, but will wait for process to finish and collect status
    pub fn status(&mut self) -> anyhow::Result<ExitStatus> {
        self.handle_redirect();
        match self.handle.spawn() {
            Ok(mut c) => {
                if let Some(buf) = self.heredoc_content.take() {
                    if let Some(mut stdin) = c.stdin.take() {
                        let _ = stdin.write_all(buf.as_bytes());
                    }
                }
                c.wait().map_err(|e| anyhow!("{}", e))
            },
            Err(e) => Err(anyhow!("{}",e)),
        }
    }
}

pub enum AstNode {
    Prog(ChildPr),

    Logical {
        lhs: Box<AstNode>,
        operator: Tkn,
        rhs: Box<AstNode>,
    },

    Pipeline(Vec<Box<AstNode>>),

    Subshell(Vec<Box<AstNode>>),
}

fn build_cmd(args: &[&str],) -> Command {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]); 
    cmd
}

pub struct Parser<'a>
{
    tkns: Peekable<Iter<'a, TknSpan>>,
    cur_tkn: Option<&'a TknSpan>,
    heredocs: VecDeque<String>,
}

impl<'a> Parser<'a> {
    pub fn new(tkns_: &'a Vec<TknSpan>, heredocs: VecDeque<String>) -> Self {
        Self {
            tkns: tkns_.iter().peekable(),
            cur_tkn: None,
            heredocs,
        }
    }

    fn advance(&mut self) -> Option<&'a TknSpan> {
        self.cur_tkn = self.tkns.next();
        self.cur_tkn
    }
    
    fn eat(&mut self, expected_type: Tkn, cmd_buf: &str) -> anyhow::Result<()>{
        if self.cur_tkn.is_none() {
            return Err(anyhow!("Syntax err: missing token {:?}", expected_type));
        }
        let cur = self.cur_tkn.unwrap();
        if cur.kind != expected_type {
            return Err(anyhow!("Syntax err: unexpected token {}", get_token_at(cur, cmd_buf)));
        }
        self.cur_tkn = self.tkns.next();
        Ok(())
    }

    fn expr(&mut self, cmd_buf: &str) -> anyhow::Result<Box<AstNode>> {
        let mut stdin = Stdio::inherit();
        let mut stdout = Stdio::inherit();
        let mut heredoc_content = None;
        let mut args = Vec::new();
        while let Some(cur_tkn) = self.advance() {
            match cur_tkn.kind {
                /* args */
                Tkn::Word | Tkn::Quote(_) => { args.push(get_token_at(cur_tkn, cmd_buf)); },
                /* backslash */
                //Tkn::Backslash => { //newline should follow backslash
                //    self.eat(Tkn::Newline, cmd_buf)?;
                //}, 
                /* redirects */
                Tkn::RedirectIn => {
                    let infile = get_token_at(self.advance().unwrap(), cmd_buf);
                    //unwrap safe because lexer and shell prompt loop guarantees a valid delimiter found
                    stdin = Stdio::from(File::open(infile)?);
                },
                Tkn::RedirectOut => {
                    let outfile = get_token_at(self.advance().unwrap(), cmd_buf);
                    stdout = Stdio::from(File::create(outfile)?);
                },
                Tkn::RedirectAppend => {
                    let outfile = get_token_at(self.advance().unwrap(), cmd_buf);
                    stdout = Stdio::from(OpenOptions::new().append(true).create(true).open(outfile)?);
                },
                Tkn::Heredoc => {
                    heredoc_content = self.heredocs.pop_front();
                    stdin = Stdio::piped();
                },
                /* Grouped commands in parentheses */ 
                Tkn::LParen => {
                    let subshell = self.subshell(cmd_buf)?;
                    self.eat(Tkn::RParen, cmd_buf)?;
                    return Ok(subshell);
                }
                /* program delimiters */
                Tkn::Newline | Tkn::CmdOr | Tkn::Pipe | Tkn::CmdAnd | Tkn::Semicolon | Tkn::RParen => {
                    if args.is_empty() { 
                        return Err(anyhow!("Syntax Err: empty args"));
                    }
                    return Ok(Box::new(AstNode::Prog(ChildPr {
                        handle: build_cmd(&args),
                        stdin: Some(stdin),
                        stdout: Some(stdout),
                        prog_name: args[0].to_string(),
                        heredoc_content,
                    })));
                },
                _ => return Err(anyhow!("Syntax Err: unexpected tkn in expression")),
            }
        }
        Err(anyhow!("no tkns"))
    }

    pub fn subshell(&mut self, cmd_buf: &str) -> anyhow::Result<Box<AstNode>> {
        let mut subsh = Vec::new();
        while self.cur_tkn.map_or(false, |tkn| tkn.kind != Tkn::RParen) {
            self.ignore_next_newlines();
            subsh.push(self.build_AST(cmd_buf)?);
        }
        Ok(Box::new(AstNode::Subshell(subsh)))
    }

    pub fn build_pipeline(&mut self, cmd_buf: &str) -> anyhow::Result<Box<AstNode>> {
        let mut node = self.expr(cmd_buf)?;

        if self.cur_tkn.map_or(false, |tkn| tkn.kind == Tkn::Pipe) {
            let mut pipeline = vec![node];
            while let Some(tkn) = self.cur_tkn {
                if tkn.kind != Tkn::Pipe { break; }
                node = self.expr(cmd_buf)?;
                pipeline.push(node);
            }
            return Ok(Box::new(AstNode::Pipeline(pipeline)))
        }
        Ok(node)
    }

    fn build_AST(&mut self, cmd_buf: &str) -> anyhow::Result<Box<AstNode>> {
        let mut node = self.build_pipeline(cmd_buf)?;
        loop {
            if self.cur_tkn.is_none() {
                //this shouldn't be reachable but just in case
                return Err(anyhow!("Syntax Err"));
            }
            let tkn = self.cur_tkn.unwrap();
            match tkn.kind {
                Tkn::Newline | Tkn::Semicolon | Tkn::RParen => return Ok(node),
                Tkn::CmdOr | Tkn::CmdAnd => {
                    node = Box::new(AstNode::Logical {
                        lhs: node,
                        operator: tkn.kind.clone(),
                        rhs: self.build_pipeline(cmd_buf)?,
                    });
                },
                _ => return Err(anyhow!("Syntax Err in build_AST\nexpected \\n, ;, ||, or && but got {:?}", tkn.kind)),
            }
        }
    }

    pub fn parse(&mut self, cmd_buf: &str) -> anyhow::Result<Vec<Box<AstNode>>> {
        let mut executables = Vec::new();
        self.ignore_next_newlines();
        while !self.tkns.peek().is_none() {
            let node = self.build_AST(cmd_buf)?;
            if self.cur_tkn.is_none() {
                //this shouldn't be reachable but just in case
                return Err(anyhow!("Syntax Err"));
            }
            let tkn = self.cur_tkn.unwrap();
            match tkn.kind {
                Tkn::Newline | Tkn::Semicolon | Tkn::RParen => {
                    executables.push(node);
                    self.ignore_next_newlines();
                }
                _ => return Err(anyhow!("Syntax err in parse\nexpected '\\n', ';', or ')', but got {:?}", tkn.kind)),
            }
        }
        Ok(executables)
    }

    fn ignore_next_newlines(&mut self) {
        while let Some(tkn) = self.tkns.peek() {
            if tkn.kind == Tkn::Newline { 
                self.tkns.next(); 
            } else {
                break;
            }
        }
    }
}



