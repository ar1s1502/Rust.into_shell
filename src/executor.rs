use crate::parser::{AstNode, Parser};
use crate::lexer::{TknSpan, Tkn};
use crate::AS_SUBSHELL;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::process::{Command, Stdio, ExitStatus};
use std::process;
use std::fs::{File, OpenOptions};
use std::io::{Write, self};
use anyhow::anyhow;

#[derive(Serialize, Deserialize)]
pub enum Redir {
    In,
    Out,
    Append,
}

#[derive(Serialize, Deserialize)]
pub struct Redirect<'a> {
    pub dir: Redir,
    pub file: &'a str,
}

#[derive(Serialize, Deserialize)]
pub struct ChildPr<'a> { //a child process spawned by shell
    #[serde(borrow)]
    pub args: Vec<&'a str>,
    //I/O streams for redirection
    pub redirect_in: Option<Redirect<'a>>, // for < operator
    pub redirect_out: Option<Redirect<'a>>, // for > 

    pub prog_name: &'a str,
    pub heredoc_content: Option<&'a str>,
}

impl<'a> ChildPr<'a> {
    fn build_cmd(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(self.args[0]);
        cmd.args(&self.args[1..]); 
        cmd
    }

    fn handle_redirect(&self, handle: &mut Command) -> anyhow::Result<()> {
        if let Some(ref infile) = self.redirect_in {
            handle.stdin(Stdio::from(File::open(infile.file)?));
        } 
        if let Some(_) = self.heredoc_content {
            handle.stdin(Stdio::piped());
        }

        if let Some(ref outfile) = self.redirect_out {
            match outfile.dir {
                Redir::Out => { handle.stdout(Stdio::from(File::create(outfile.file)?)); },
                Redir::Append => { handle.stdout(Stdio::from(OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(outfile.file)?)); 
                },
                _ => return Err(anyhow!("unreachable")),
            }
        }
        Ok(())
    }

    pub fn spawn(
        &mut self,
        stdin: Stdio,
        stdout: Stdio,
    ) -> anyhow::Result<process::Child> {
        let mut handle = self.build_cmd();
        handle.stdin(stdin);
        handle.stdout(stdout);
        self.handle_redirect(&mut handle)?;
        match handle.spawn() {
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
        let mut handle = self.build_cmd();
        self.handle_redirect(&mut handle)?;
        match handle.spawn() {
            Ok(mut c) => {
                if let Some(buf) = self.heredoc_content.take() {
                    if let Some(mut stdin) = c.stdin.take() {
                        let _ = stdin.write_all(buf.as_bytes());
                    }
                }
                Ok(c.wait()?)
            },
            Err(e) => Err(anyhow!("{}",e)),
        }
    }
}

fn spawn_pipeline(progs: &mut Vec<Box<AstNode>>) -> anyhow::Result<Vec<process::Child>> {
    let num_prog = progs.len();
    let mut children = Vec::with_capacity(num_prog);
    let mut cur_stdin = Stdio::inherit();
    for (i, prog) in progs.iter_mut().enumerate() {
        let last_child = i == num_prog - 1;
        let (next_stdin, child_stdout) = if last_child {
            (Stdio::inherit(), Stdio::inherit())
        } else {
            let (pipe_reader, pipe_writer) = io::pipe()?;
            (Stdio::from(pipe_reader), Stdio::from(pipe_writer))
        };
        //prog is either a Prog(child_pr) or a Subshell(subsh)
        match &mut **prog {
            AstNode::Prog(child_pr) => {
               let child = child_pr.spawn(cur_stdin, child_stdout)?;
               children.push(child);
            },
            AstNode::Subshell(inner_ast) => {
                let shell_exe = std::env::current_exe()?;
                let serialized_ast = serde_json::to_string(inner_ast)?;
                let subsh = Command::new(shell_exe)
                    .env(AS_SUBSHELL, &serialized_ast)
                    .stdin(cur_stdin)
                    .stdout(child_stdout)
                    .spawn()?;
                children.push(subsh);
            },
            _ => return Err(anyhow!("unreachable, pipe can only have Prog or Subshell")),
        }
        cur_stdin = Stdio::from(next_stdin);
    }
    Ok(children)
}

// return the exit code of executing the ast node
fn dfs(node: &mut Box<AstNode>) -> anyhow::Result<i32> {
    match &mut **node {
        AstNode::Prog(child_pr) => {
            return Ok(child_pr.status()?
                .code()
                .unwrap_or(1));
        },
        AstNode::Logical { 
            lhs, 
            operator, 
            rhs 
        } => {
            let lhs_code = dfs(lhs)?;
            match operator {
                Tkn::CmdOr => {
                    if lhs_code != 0 { 
                        return dfs(rhs);
                    } 
                    return Ok(0);
                },
                Tkn::CmdAnd => {
                    if lhs_code == 0 {
                        return dfs(rhs);
                    } 
                    return Ok(lhs_code);
                },
                _ => return Err(anyhow!("unreachable; invalid op in Logical astnode"))
            }
        },
        AstNode::Pipeline(pipeline) => {
            let mut spawned_children = spawn_pipeline(pipeline)?;
            let mut res = 0;
            for c in spawned_children.iter_mut() {
                res = c.wait()?
                    .code()
                    .unwrap_or(1);
                if res != 0 { break; } 
            }
            return Ok(res);
        },
        AstNode::Subshell(inner_ast) => {
            let shell_path = std::env::current_exe().expect("Failed to get current exe path for subshell");
            let serialized_ast = serde_json::to_string(inner_ast)?;
            let subsh_stat = Command::new(shell_path)
                .env(AS_SUBSHELL, &serialized_ast)
                .status()?;
            return Ok(subsh_stat.code().unwrap_or(1));
        },
    }
}

pub fn execute_cmd_buf<'w> (cmd_buf: &'w str, tkns: &'w [TknSpan], heredocs: VecDeque<&'w str>) -> anyhow::Result<i32> {
    let executables = Parser::new(tkns, heredocs).parse(cmd_buf)?;
    execute_ast(executables)
}

pub fn execute_ast(mut executables: Vec<Box<AstNode>>) -> anyhow::Result<i32> {
    let mut res = 0;
    for ast in executables.iter_mut() {
        res = dfs(ast)?;
    }
    Ok(res)
}







