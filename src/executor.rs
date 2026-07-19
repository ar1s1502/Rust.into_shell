use crate::parser::{AstNode, Parser};
use crate::lexer::{TknSpan, Tkn};
use crate::{AS_SUBSHELL, RL_EDITOR, is_debug, exit_shell};
use serde::{Deserialize, Serialize};
use std::collections::{VecDeque, HashMap};
use std::process::{Command, ExitStatus, Stdio};
use std::process;
use std::fs::{File, OpenOptions, };
use std::io::{Write, self, PipeReader, PipeWriter, Read};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use anyhow::anyhow;
use rustyline::history::{History, SearchDirection};

type BuiltinFn = fn(&[&str], Option<PipeReader>) -> anyhow::Result<String>;

//Global immutable hashmap of <builtin command name>:<function to execute builtin>
pub static BUILTINS: OnceLock<HashMap<&'static str, BuiltinFn>> = OnceLock::new();
pub fn get_builtins() -> &'static HashMap<&'static str, BuiltinFn> {
    BUILTINS.get_or_init(|| {
        HashMap::from([
            ("pwd", pwd as BuiltinFn),
            ("cd", set_cwd),
            ("history", get_history),
            ("exit", exit_shell),
        ])
    })
}

#[derive(Serialize, Deserialize, )]
pub enum Redir {
    In,
    Out,
    Append,
    Heredoc,
}

#[derive(Serialize, Deserialize, )]
pub struct Redirect {
    pub dir: Redir,
    pub file: String,
}

#[derive(Serialize, Deserialize)]
pub struct Builtin<'a> { //built in command 
    #[serde(borrow)]
    pub args: Vec<&'a str>,
    //I/O streams for redirection
    pub redirect_ins: Vec<Redirect>,
    pub redirect_outs: Vec<Redirect>,
                                            
    //pub heredoc_content: Vec<'a str>, 
}

impl<'a> Builtin<'a> {
    pub fn exec_builtin(&self, pipe_write: Option<PipeWriter>, pipe_read: Option<PipeReader>) {
        let builtin_fn = get_builtins().get(self.args[0]).unwrap(); //unwrap safe because parser checks if in builtins
        match builtin_fn(&self.args, pipe_read) {
            Ok(output_str) => {
                if let Some(mut pipe_writer) = pipe_write {
                    std::thread::spawn(move || {
                        let _ = pipe_writer.write_all(output_str.as_bytes());
                    });
                } else {
                    println!("{}", output_str);
                }
            },
            Err(e) => println!("{}", e),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ChildPr<'a> { //a child process spawned by shell
    #[serde(borrow)]
    pub args: Vec<&'a str>,
    //I/O streams for redirection
    pub redirect_ins: Vec<Redirect>,
    pub redirect_outs: Vec<Redirect>,

    pub prog_name: &'a str,
    pub heredoc_contents: VecDeque<String>,
}

impl<'a> ChildPr<'a> {
    pub fn spawn(&mut self, stdin: Stdio, stdout: Stdio) 
    -> anyhow::Result<process::Child> {
        let mut handle = Command::new(self.args[0]);
        handle.args(&self.args[1..]); 
        handle.stdin(stdin);
        handle.stdout(stdout);
        self.apply_redirect(&mut handle)
    }

    //same as spawn, but will wait for process to finish and collect status
    pub fn status(&mut self) -> anyhow::Result<ExitStatus> {
        let mut c = self.spawn(Stdio::inherit(), Stdio::inherit())?;
        Ok(c.wait()?)
    }

    //spawn the child process, apply any redirect operators (<, <<, >, >>)
    fn apply_redirect(&mut self, handle: &mut process::Command) -> anyhow::Result<process::Child> {
        if !self.redirect_ins.is_empty() { handle.stdin(Stdio::piped()); };
        if !self.redirect_outs.is_empty() { handle.stdout(Stdio::piped()); };
        let mut c = handle.spawn()?;
        if !self.redirect_ins.is_empty() {
            let mut stdin_handle = c.stdin.take().expect("Failed to take child stdin handle");
            let redirects = std::mem::take(&mut self.redirect_ins);
            let mut heredocs = std::mem::take(&mut self.heredoc_contents);
            std::thread::spawn(move || {
                for r in redirects.into_iter() {
                    match r.dir {
                        Redir::Heredoc => { 
                            if let Some(ref content) = heredocs.pop_front() { 
                                //write_all is fine because different thread 
                                let _ = stdin_handle.write_all(content.as_bytes());
                            }
                        },
                        Redir::In => {
                            if let Ok(mut f) = File::open(&r.file) {
                                //write to child stdin in chunks until f's eof
                                let _ = std::io::copy(&mut f, &mut stdin_handle);
                            }
                        }
                        _ => (),
                    }
                }
            });
        }
        if !self.redirect_outs.is_empty() {
            let mut stdout_handle = c.stdout.take().expect("Failed to take child stdout handle");
            let redirects = std::mem::take(&mut self.redirect_outs);
            let mut outfiles = Vec::new();
            //create/open all outfiles. (this happens even if command stdout is never written to)
            for r in redirects.iter() {
                match r.dir {
                    Redir::Out => outfiles.push(OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(&r.file)?), //This should be equivalent to File::create
                    Redir::Append => outfiles.push(OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&r.file)?),
                    _ => anyhow::bail!("unreachable: got redirect in while executing redirect out"),
                }
            }
            std::thread::spawn(move || {
                let mut buf = [0u8; 5*(1<<10)];
                loop {
                    match stdout_handle.read(&mut buf) {
                        Ok(0) => break, //EOF
                        Ok(n) => {
                            for f in outfiles.iter_mut() {
                                let _ = f.write_all(&buf[..n]);
                            }
                        },
                        Err(_) => break, 
                    }
                }
            });            
        }

        Ok(c) 
    }

}

//convert std::io::PipeWriter/Reader to std::process::Stdio
fn convert_pipe_fds(pipe_w: Option<PipeWriter>, pipe_r: Option<PipeReader>) -> (Stdio, Stdio) {
    (
        match pipe_w {
            None => Stdio::inherit(),
            Some(w_fd) => Stdio::from(w_fd),
        },
        match pipe_r {
            None => Stdio::inherit(),
            Some(r_fd) => Stdio::from(r_fd),
        }
    )
}

fn spawn_pipeline(progs: &mut Vec<Box<AstNode>>) -> anyhow::Result<Vec<process::Child>> {
    let num_prog = progs.len();
    let mut children = Vec::with_capacity(num_prog);
    let mut cur_pipe_read: Option<PipeReader> = None;
    for (i, prog) in progs.iter_mut().enumerate() {
        let last_child = i == num_prog - 1;
        let (next_pipe_read, cur_pipe_write) = 
            if last_child {
                (None, None)
            } else {
                let (pipe_reader, pipe_writer) = io::pipe()?;
                (Some(pipe_reader), Some(pipe_writer))
            };
        //prog is either a Prog(child_pr) or a Subshell(subsh)
        match &mut **prog {
            AstNode::Builtin(builtin) => {
                builtin.exec_builtin(cur_pipe_write, cur_pipe_read);
            },
            AstNode::Prog(child_pr) => {
                let (c_stdout, c_stdin) = convert_pipe_fds(cur_pipe_write, cur_pipe_read);

                let child = child_pr.spawn(c_stdin, c_stdout)?;
                children.push(child);
            },
            AstNode::Subshell(inner_ast) => {
                let (c_stdout, c_stdin) = convert_pipe_fds(cur_pipe_write, cur_pipe_read);

                let shell_exe = std::env::current_exe()?;
                let serialized_ast = serde_json::to_string(inner_ast)?;
                let subsh = Command::new(shell_exe)
                    .env(AS_SUBSHELL, &serialized_ast)
                    .stdin(c_stdin)
                    .stdout(c_stdout)
                    .spawn()?;
                children.push(subsh);
            },
            _ => return Err(anyhow!("unreachable, pipe can only have Prog or Subshell")),
        }
        cur_pipe_read = next_pipe_read;
    }
    Ok(children)
}

// return the exit code of executing the ast node
fn dfs(node: &mut Box<AstNode>) -> anyhow::Result<i32> {
    match &mut **node {
        AstNode::Builtin(builtin) => {
            builtin.exec_builtin(None, None);
            return Ok(0);
        }
        AstNode::Prog(child_pr) => {
            if child_pr.args.is_empty() { return Ok(0); }
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
            if spawned_children.is_empty() { return Ok(0); }
            let last = spawned_children.len() - 1;
            for (i, c) in spawned_children.iter_mut().enumerate() {
                if i == last {
                    if let Ok(exit_stat) = c.wait() {
                        return Ok(exit_stat.code().unwrap_or(1));
                    }
                    return Ok(1)
                } else {
                    let _ = c.wait();
                }
            }
            return Ok(0);
        },
        AstNode::Subshell(inner_ast) => {
            let shell_path = env::current_exe().expect("Failed to get current exe path for subshell");
            let serialized_ast = serde_json::to_string(inner_ast)?;
            let subsh_stat = Command::new(shell_path)
                .env(AS_SUBSHELL, &serialized_ast)
                .status()?;
            return Ok(subsh_stat.code().unwrap_or(1));
        },
    }
}

pub fn execute_cmd_buf<'w> (cmd_buf: &'w str, tkns: &'w [TknSpan], heredocs: VecDeque<&'w str>) -> anyhow::Result<i32> {
    let executables = Parser::new(tkns, heredocs, cmd_buf).parse()?;
    if is_debug() { println!("\nOUTPUT!!"); }
    execute_ast(executables)
}

pub fn execute_ast(mut executables: Vec<Box<AstNode>>) -> anyhow::Result<i32> {
    let mut res = 0;
    for ast in executables.iter_mut() {
        res = dfs(ast)?;
    }
    Ok(res)
}

/* BUILTINS */
fn pwd(_args: &[&str], _pipe_reader: Option<PipeReader>) -> anyhow::Result<String> { 
    Ok(format!("{}", env::current_dir().unwrap().display()))
}

fn set_cwd(args: &[&str], _pipe_reader: Option<PipeReader>) -> anyhow::Result<String> {
    if args.len() == 1 { //cd 
        let home = env::home_dir().expect("ERR cd: Couldn't find HOME directory");
        env::set_current_dir(&home)?;
        return Ok("".to_string());
    } else if args.len() == 2 { //cd [..]
        let path_str = args[1];
        let mut new_cwd = PathBuf::from(Path::new(path_str));
        if new_cwd.starts_with("~") {
            new_cwd = expand_tilde(&new_cwd);
        }
        env::set_current_dir(&new_cwd)?;
        return Ok("".to_string());
    } else if args.len() > 2 {
        return Err(anyhow!("ERR cd: too many arguments for cd; {} is invalid", args[2]));
    }
    Err(anyhow!("unreachable"))
} 

//TODO: fix
fn expand_tilde(path: &PathBuf) -> PathBuf {
    let mut expanded_path = env::home_dir().expect("Couldn't find HOME directory");
    if path == Path::new("~") {
        expanded_path
    } else {
        expanded_path.push(path.strip_prefix("~").unwrap());
        expanded_path
    }
}

fn get_history(args: &[&str], _pipe_reader: Option<PipeReader>) -> anyhow::Result<String> {
    let mut output = String::new();
    if args.len() > 1 { 
        match args[1].to_lowercase().as_ref() {
            "clear" => {
                let success = RL_EDITOR.with_borrow_mut(|rl| { rl.history_mut().clear() }).is_ok();
                if success { return Ok("command history cleared".to_string()) } else { return Err(anyhow!("Failed to clear history")) }
            }
            _ => return Err(anyhow!("unrecognized history parameter {}", args[1])),
        };
    }
    let hist_len = RL_EDITOR.with_borrow(|h| h.history().len());
    let start = std::cmp::max(0, hist_len - 15);
    for i in start..hist_len {
        RL_EDITOR.with_borrow(|rl| {
            let entry = rl.history().get(i, SearchDirection::Forward).unwrap().unwrap().entry;
            if i != hist_len - 1 {
                output.push_str(&format!("{}\n", entry));
            } else {
                output.push_str(&entry);
            }
        })
    }
    Ok(output)
}

