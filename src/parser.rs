use crate::lexer::{TknSpan, Tkn, get_token_at};
use crate::executor::{ChildPr, Redirect, Redir, };
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
    
    fn eat(&mut self, expected_type: Tkn, cmd_buf: &'a str) -> anyhow::Result<()> {
        if let Some(tkn) = self.advance() {
            if tkn.kind != expected_type {
                return Err(anyhow!("Syntax err: unexpected token {}", get_token_at(tkn, cmd_buf)));
            }
        } else {
            return Err(anyhow!("Syntax err: missing token {:?}", expected_type));
        }
        Ok(())
    }

    fn expr(&mut self, cmd_buf: &'a str) -> anyhow::Result<Box<AstNode<'a>>> {
        let mut redirect_in = None;
        let mut redirect_out = None;
        let mut heredoc_content = None;
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
                    heredoc_content = self.heredocs.pop_front();
                    //eat the heredoc delimiter
                    match self.tkns.peek().map(|t| &t.kind) {
                        Some(Tkn::Word) => { 
                            self.eat(Tkn::Word, cmd_buf)?; 
                        }
                        Some(Tkn::Quote(s)) => { 
                            self.eat(Tkn::Quote(s.clone()), cmd_buf)?; 
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
                    let subshell = self.subshell(cmd_buf)?;
                    self.eat(Tkn::RParen, cmd_buf)?;
                    return Ok(subshell);
                }
                /* program delimiters */
                Tkn::Newline | Tkn::CmdOr | Tkn::Pipe | Tkn::CmdAnd | Tkn::Semicolon | Tkn::RParen => {
                    if args.is_empty() { 
                        return Err(anyhow!("Syntax Err: empty args"));
                    }
                    println!("args from parser: {:?}", args);
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
        while self.cur_tkn.map_or(false, |tkn| tkn.kind != Tkn::RParen) {
            self.ignore_next_newlines();
            subsh.push(self.build_ast(cmd_buf)?);
        }
        Ok(Box::new(AstNode::Subshell(subsh)))
    }

    fn build_pipeline(&mut self, cmd_buf: &'a str) -> anyhow::Result<Box<AstNode<'a>>> {
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
                _ => return Err(anyhow!("Syntax Err in build_ast\nexpected \\n, ;, ||, or && but got {:?}", tkn.kind)),
            }
        }
    }

    pub fn parse(&mut self, cmd_buf: &'a str) -> anyhow::Result<Vec<Box<AstNode<'a>>>> {
        let mut executables = Vec::new();
        self.ignore_next_newlines();
        while !self.tkns.peek().is_none() {
            let node = self.build_ast(cmd_buf)?;
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
                _ => return Err(anyhow!("Syntax Err:\nwhile parsing, expected '\\n', ';', or ')', but got {:?}", tkn.kind)),
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



