use crate::lexer::{TknSpan, Tkn, get_token_at};
use crate::executor::{ChildPr, Builtin, Redirect, Redir, get_builtins};
use std::collections::VecDeque;
use std::iter::{Peekable};
use std::slice::Iter;
use serde::{Deserialize, Serialize};
use anyhow::anyhow;

/* 
 * Recursive Descent Parser
 * See https://ruslanspivak.com/lsbasi-part7/ for an e.g.
 *
 * */

#[derive(Serialize, Deserialize)]
pub enum AstNode<'a> {
    #[serde(borrow)]
    Prog(ChildPr<'a>),

    Logical {
        lhs: Box<AstNode<'a>>,
        operator: Tkn,
        rhs: Box<AstNode<'a>>,
    },

    Pipeline(Vec<Box<AstNode<'a>>>),

    Subshell(Vec<Box<AstNode<'a>>>),

    Builtin(Builtin<'a>),
}

pub struct Parser<'a>
{
    tkns: Peekable<Iter<'a, TknSpan>>,
    cur_tkn: Option<&'a TknSpan>,
    heredocs: VecDeque<&'a str>,
}

impl<'a> Parser<'a> {
    pub fn new(tkns_: &'a [TknSpan], heredocs: VecDeque<&'a str>) -> Self {
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
    
    fn eat(&mut self, expected_type: Tkn) -> anyhow::Result<()> {
        if let Some(tkn) = self.advance() {
            if tkn.kind != expected_type {
                return Err(anyhow!("Syntax err: expected token of kind {} but got '{}'", expected_type, tkn.kind));
            }
        } else {
            return Err(anyhow!("Syntax err: missing token {:?}", expected_type));
        }
        Ok(())
    }

    fn expr(&mut self, cmd_buf: &'a str) -> anyhow::Result<Box<AstNode<'a>>> {
        let mut redirect_in = None;
        let mut redirect_out = None;
        let mut heredoc_content = Vec::new();
        let mut args = Vec::new();
        while let Some(cur_tkn) = self.advance() {
            match cur_tkn.kind {
                /* args */
                Tkn::Word | Tkn::Quote(_) => { args.push(get_token_at(cur_tkn, cmd_buf)); },
                /* backslash */
                //Tkn::Backslash => { //newline should follow backslash
                //}, 
                /* redirects */ 
                Tkn::RedirectIn => {
                    let infile = get_token_at(self.advance().unwrap(), cmd_buf);
                    //unwrap safe because lexer and shell prompt loop guarantees a valid delimiter found
                    redirect_in = Some(Redirect { dir: Redir::In, file: infile });
                },
                Tkn::RedirectOut => {
                    let outfile = get_token_at(self.advance().unwrap(), cmd_buf);
                    redirect_out = Some(Redirect { dir: Redir::Out, file: outfile });
                },
                Tkn::RedirectAppend => {
                    let outfile = get_token_at(self.advance().unwrap(), cmd_buf);
                    redirect_out = Some(Redirect { dir: Redir::Append, file: outfile });
                },
                Tkn::Heredoc => {
                    heredoc_content.push(self.heredocs.pop_front().unwrap_or(""));
                    //eat the heredoc delimiter
                    match self.tkns.peek().map(|t| &t.kind) {
                        Some(Tkn::Word) => { 
                            self.eat(Tkn::Word)?; 
                        }
                        Some(Tkn::Quote(s)) => { 
                            self.eat(Tkn::Quote(s.clone()))?; 
                        }
                        Some(_) => {
                            anyhow::bail!("unreachable: Invalid delimiter for heredoc");
                        }
                        None => {
                            anyhow::bail!("unreachable: Expected heredoc delimiter, found EOF");
                        }
                    };
                },
                /* Grouped commands in parentheses */ 
                Tkn::LParen => {
                    return self.subshell(cmd_buf);
                }
                /* program delimiters */
                Tkn::Newline | Tkn::Semicolon | Tkn::CmdOr | Tkn::CmdAnd | Tkn::RParen | Tkn::Pipe => {
                    if args.is_empty() { 
                        return Err(anyhow!("Syntax Err: empty args"));
                    }
                    println!("args from parser: {:?}", args);
                    //if args[0] is a builtin command, then return astnode::builtin
                    if get_builtins().get(args[0]).is_some() {
                        return Ok(Box::new(AstNode::Builtin(Builtin {
                            args,
                            redirect_in,
                            redirect_out,
                        })));
                    }
                    return Ok(Box::new(AstNode::Prog(ChildPr {
                        prog_name: args[0],
                        args: args,
                        redirect_in,
                        redirect_out,
                        heredoc_content,
                    })));
                },
                _ => return Err(anyhow!("Syntax Err: unexpected tkn in expression")),
            }
        }
        Err(anyhow!("Parse error: no tkns"))
    }

    fn subshell(&mut self, cmd_buf: &'a str) -> anyhow::Result<Box<AstNode<'a>>> {
        let mut subsh = Vec::new();
        while self.cur_tkn.map_or(false, |t| t.kind != Tkn::RParen) {
            subsh.push(self.build_ast(cmd_buf)?);
            //build ast stops at a newline, semicolon, or rparen
            if self.cur_tkn.map_or(false, |t| t.kind == Tkn::RParen) { break; }
            self.ignore_next_program_delims();
            if self.tkns.peek().map_or(false, |t| t.kind == Tkn::RParen) { 
                //found the closing paren for subshell
                self.eat(Tkn::RParen)?;
            }
        }
        Ok(Box::new(AstNode::Subshell(subsh)))
    }

    fn build_pipeline(&mut self, cmd_buf: &'a str) -> anyhow::Result<Box<AstNode<'a>>> {
        let mut node = self.expr(cmd_buf)?;
        if self.cur_tkn.map_or(false, |tkn| tkn.kind == Tkn::Pipe) {
            let mut pipeline = vec![node];
            while let Some(tkn) = self.cur_tkn {
                if tkn.kind != Tkn::Pipe { break; }
                self.ignore_next_program_delims();
                println!("cur tkn: {}", self.cur_tkn.unwrap().kind);
                node = self.expr(cmd_buf)?;
                pipeline.push(node);
            }
            return Ok(Box::new(AstNode::Pipeline(pipeline)))
        }
        Ok(node)
    }

    fn build_ast(&mut self, cmd_buf: &'a str) -> anyhow::Result<Box<AstNode<'a>>> {
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
                _ => return Err(anyhow!("Syntax Err in build_ast\nexpected \\n, ;, ||, or && but got '{}'", tkn.kind)),
            }
        }
    }

    pub fn parse(&mut self, cmd_buf: &'a str) -> anyhow::Result<Vec<Box<AstNode<'a>>>> {
        let mut executables = Vec::new();
        self.ignore_next_program_delims();
        while self.tkns.peek().is_some() {
            let node = self.build_ast(cmd_buf)?;
            if self.cur_tkn.is_none() {
                //this shouldn't be reachable but just in case
                return Err(anyhow!("Syntax Err"));
            }
            let tkn = self.cur_tkn.unwrap();
            match tkn.kind {
                Tkn::Newline | Tkn::Semicolon | Tkn::RParen => {
                    executables.push(node);
                    self.ignore_next_program_delims();
                }
                _ => return Err(anyhow!("Syntax Err:\nwhile parsing, expected '\\n', ';', or ')', but got '{}'", tkn.kind)),
            }
        }
        Ok(executables)
    }

    fn ignore_next_program_delims(&mut self) {
        while let Some(tkn) = self.tkns.peek() {
            if [Tkn::Newline, Tkn::Semicolon,].contains(&tkn.kind) {
                self.advance();
            } else { break; }
        }
    }

}



