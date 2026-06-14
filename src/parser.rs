use crate::lexer::{TknSpan, Tkn, get_token_at};
use std::process::{Command, Stdio, ExitStatus};
use std::fs::{File, OpenOptions};
use anyhow::anyhow;

struct ChildPr { //a child process spawned by shell
    pub handle: Command,
    //I/O streams for redirection
    stdin: Stdio,
    stdout: Stdio,

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
    pub fn status(&mut self) -> anyhow::Result<process::ExitStatus> {
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

pub enum AST_node {
    Prog(ChildPr),

     Logical {
        lhs: Box<AST_node>,
        operator: Tkn,
        rhs: Box<AST_node>,
    },

    Pipeline(Vec<ChildPr>),

    Group(AST_node),
}

fn parse_program(tkns: Vec<TknSpan>, cmd_buf: &str, heredocs: &mut VecDeque<String>) -> Result<ChildPr, std::io::Error> {
    let mut stdin = Stdio::inherit();
    let mut stdout = Stdio::inherit();
    let mut heredoc_content = Some(String);
    let mut args = Vec::new();
    let mut i = 0;
    let tkn_iter = tkns.iter();
    while let Some(tkn_span) = tkn_iter.next() {
        match tkn_span.kind {
            //arg
            Tkn::Word | Tkn::Quote(_) => args.push(get_token_at(tkn_span, cmd_buf)),
            //redirect
            Tkn::RedirectIn => {
                let infile = get_token_at(tkn_iter.next().unwrap(), cmd_buf);
                //unwrap safe because lexer and shell prompt loop guarantees a valid delimiter found
                stdin = Stdio::from(File::open(infile)?);
            },
            Tkn::RedirectOut => {
                let outfile = get_token_at(tkn_iter.next().unwrap(), cmd_buf);
                stdout = Stdio::from(File::create(outfile)?);
            },
            Tkn::RedirectAppend => {
                let outfile = get_token_at(tkn_iter.next().unwrap(), cmd_buf);
                stdout = Stdio::from(OpenOptions::new().append(true).create(true).open(outfile)?);
            },
            Tkn::Heredoc => {
                heredoc_content = heredocs.pop_front();
                stdin = Stdio::piped();
            },
            //program delimiter
            Tkn::Newline => {
                
            }

        }
    }


    match split(prog) {
        Ok(t) => tkns = t,
        Err(e) => return Err(anyhow!("{}", e)),
    }
    while i < tkns.len() {
        match tkns[i].as_str() {
            ">" => {
                i += 1;
                match File::create(tkns[i].as_str()) {
                    Ok(f) => stdout = Some(Stdio::from(f)),
                    Err(e) => return Err(anyhow!("{}", e)),
                };
            },
            "<" => {
                i += 1;
                match File::open(tkns[i].as_str()) {
                    Ok(f) => stdin = Some(Stdio::from(f)),
                    Err(e) => return Err(anyhow!("{}", e)),
                };
            },
            ">>" => {
                i += 1;
                match OpenOptions::new().append(true).create(true).open(tkns[i].as_str()) {
                    Ok(f) => stdout = Some(Stdio::from(f)),
                    Err(e) => return Err(anyhow!("{}", e)),
                };
            },
            "<<" => {
                i += 1;
                if tkns.get(i).is_none() {
                    return Err(anyhow!("please specify a delimiter"));
                }
                if let Some(heredoc) = heredocs.pop_front() {
                    heredoc_content.push_str(&heredoc);
                } else {
                    return Err(anyhow!("no heredoc body specified"));
                }
                stdin = Some(Stdio::piped());
            }
            _ => {
                args.push(&tkns[i]);
            },
        }
        i += 1;
    };
    let mut cmd = process::Command::new(args[0].clone());
    cmd.args(&args[1..]); 
    
    Ok(ChildPr {
        handle: cmd,
        stdin: stdin,
        stdout: stdout,
        prog_name: args[0].clone(),
        heredoc_content: if heredoc_content.is_empty() { None } else { Some(heredoc_content) }
    })
}


pub fn build_AST(tkns: &Vec<TknSpan>, cmd_buf: &str, heredocs: &mut VecDeque<String>) {
    
}

