mod lexer;

use lexer::{Tkn, LexerState};
use logos::{Logos, Lexer};
use rustyline::{DefaultEditor};
use rustyline::error::ReadlineError;
use rustyline::history::{History,};
use std::process::Stdio;
use std::{env, process};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::io::Write;
use std::collections::VecDeque;
use shellwords::{split};
use anyhow::anyhow;

const EDITOR_HISTORY: &str = "./shell_cmd_history.txt";

struct ChildPr { //a child process spawned by shell
    pub handle: process::Command,
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

/* 
create a Vec<Child> children
For each program prog in line.split('|'):
    Use Command::new(prog).spawn to create a new child process, child
    child.stdout = Stdio::inherit() (aka console) if is last program, else = Stdio::piped() 
    child.stdin = Stdio::null() if is first program, else is children[i-1].stdout.unwrap();
    append the child process to children vec
then for each child in children, wait on the child.
automatically the console should have the stdout of the last prog
 * */

fn handle_cmd(cmd: &str, heredocs: &mut VecDeque<String>) {
    let progs: Vec<&str> = cmd
        .split('|')
        .map(|prog| prog.trim())
        .filter(|&prog| !prog.is_empty())
        .collect(); 
    //only one program to execute
    if progs.len() == 1 {
        if progs[0].trim().is_empty() { return; }
        match parse_program(progs[0], heredocs) {
            Ok(mut child_pr) =>{
                match child_pr.prog_name.as_str() {
                    "?" => println!("help:"),
                    "pwd" => println!("{}", env::current_dir().unwrap().display()),
                    "cd" => match split(progs[0]) {
                        Ok(args) => set_cwd(&args),
                        Err(e) => println!("ERR: {}", e),
                    },
                    _ => match child_pr.status() {
                        Ok(_) => (),
                        Err(e) => println!("ERR: {}", e),
                    },
                }
            },
            Err(e) => println!("ERR: {}", e),
        }
        return;
    }
    //multiple programs, so set up pipes and fork processes
    let mut children: Vec<process::Child> = Vec::with_capacity(progs.len());
    let mut cur_child: ChildPr;
    let mut child_prs: Vec<ChildPr> = Vec::with_capacity(progs.len());
    let mut pipe_setup_success = true;
    for (i, prog) in progs.iter().enumerate() {
        if (*prog).trim().is_empty() { 
            continue; 
        }
        match parse_program(*prog, heredocs) {
            Ok(child) => cur_child = child,
            Err(e) => {
                println!("ERR {}", e);
                continue;
            }
        }
        if i != progs.len() - 1 {
            cur_child.set_stdout(Stdio::piped());
        }
        if i != 0 {
            if let Some(prev_prog) = children.get_mut(i - 1) {
                cur_child.set_stdin(Stdio::from(prev_prog.stdout.take().expect("Failed to open stdout")));
            } else {
                pipe_setup_success = false;
            }
        }
        match cur_child.spawn() {
            Ok(c) => children.push(c),
            Err(e) => println!("ERR: {}", e),
        }   
        child_prs.push(cur_child);
    }
    
    if pipe_setup_success {
        children.iter_mut().map(|child| child.wait()).for_each(move |res| {
            match res {
                Ok(_) => (),
                Err(e) => println!("ERR: {}", e),
            }
        }); 
    } else {
        children.iter_mut().map(|child| child.kill()).for_each(move |res| {
            if let Err(e) = res {
                println!("ERR: {}", e);
            }
        });
    }

}

//Parser
fn parse_program(prog: &str, heredocs: &mut VecDeque<String>) -> anyhow::Result<ChildPr> {
    let tkns: Vec<String>;
    match split(prog) {
        Ok(t) => tkns = t,
        Err(e) => return Err(anyhow!("{}", e)),
    }
    let mut stdin = None;
    let mut stdout = None;
    let mut heredoc_content = String::new();
    let mut args = Vec::new();
    let mut i = 0;
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

fn set_cwd(args: &Vec<String>) {
    if args.len() == 1 { //cd 
        let home = env::home_dir().expect("Couldn't find HOME directory");
        env::set_current_dir(&home).expect("ERR chdir");
    } else if args.len() == 2 { //cd [..]
        let mut new_cwd = PathBuf::from(Path::new(&args[1]));
        if new_cwd.starts_with("~") {
            new_cwd = expand_tilde(&new_cwd);
        }
        if let Err(_) = env::set_current_dir(&new_cwd) {
            println!("ERR: {} is an invalid path", new_cwd.display());
        }
    } else {
        println!("ERR: cd: too many arguments");
    }
} 

fn expand_tilde(path: &PathBuf) -> PathBuf {
    let mut expanded_path = env::home_dir().expect("Couldn't find HOME directory");
    if path == Path::new("~") {
        expanded_path
    } else {
        expanded_path.push(path.strip_prefix("~").unwrap());
        expanded_path
    }
}

fn lex_cmd_buf(lex: &mut Lexer<Tkn>) -> Option<(usize, VecDeque<String>, String)> {
    let mut cmd_length: usize = 0;
    while let Some(res) = lex.next() {
        match res {
            Ok(Tkn::Newline((heredocs, cmd_continuation))) => {
                //first newline encountered finishes the command. (User pressed enter)
                print!("\n");
                cmd_length += 1; //+1 for newline
                return Some((cmd_length, heredocs, cmd_continuation));
            },
            Ok(Tkn::Quote(quoted_string)) => {
                print!("tkn: \"{}\"; ", &quoted_string);
                cmd_length += lex.slice().len();
            },
            Ok(Tkn::Whitespace) => cmd_length += lex.slice().len(),
            Ok(_) =>  {
                print!("tkn: {}; ", lex.slice());
                cmd_length += lex.slice().len();
            },
            Err(_) => return None,
        }
    }
    None
}

fn main() -> rustyline::Result<()> {
    let mut rl = DefaultEditor::new()?;
    if rl.history_mut().load(Path::new(EDITOR_HISTORY)).is_err() {
        println!("Failed to load shell command history");
    }
    let mut cmd_buf = String::new();
    let mut line_num = 0;
    let mut prompt = String::new();
    set_normal_prompt(&mut prompt, &line_num);

    loop {
        line_num += 1;
        match rl.readline(&prompt) {
            Ok(input) => {
                if input.is_empty() {continue;}
                if input.trim() == "exit" { exit_shell(&mut rl); };
                cmd_buf.push_str(&input);
                cmd_buf.push('\n'); //add back newline that readline stripped after user hit Enter
                let lex_state = LexerState::new();
                let mut lex = Tkn::lexer_with_extras(&cmd_buf, lex_state);
                match lex_cmd_buf(&mut lex) {
                    Some((cmd_length, mut heredocs, cmd_continuation)) => {
                        let full_cmd = format!("{} {}", &cmd_buf[..cmd_length], &cmd_continuation);
                        handle_cmd(&full_cmd, &mut heredocs);
                        set_normal_prompt(&mut prompt, &line_num);
                        let _ = rl.add_history_entry(cmd_buf.trim());
                        cmd_buf.clear();
                    },
                    None => {
                        if let Some(err) = lex.extras.syntax_err {
                            //syntax errs get highest priority b/c they're unrecoverable
                            println!("Syntax ERR: {}", err);
                            set_normal_prompt(&mut prompt, &line_num);
                            let _  = rl.add_history_entry(cmd_buf.trim());
                            cmd_buf.clear();
                        } else if let Some(closer) = lex.extras.expected_closer {
                            set_expected_closer_prompt(&mut prompt, &closer);
                        } else if let Some(op) = lex.extras.continuation_for {
                            set_needs_continuation_prompt(&mut prompt, &op);
                        }
                    }
                }
            },
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                set_normal_prompt(&mut prompt, &line_num);
                cmd_buf.clear();
            },
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                set_normal_prompt(&mut prompt, &line_num);
                cmd_buf.clear();
            },
            Err(err) => {
                println!("ERR: {:?}", err);
            },
        }
    }
}

fn set_normal_prompt(prompt: &mut String, line_num: &usize) {
    let cwd = env::current_dir().unwrap().file_name().unwrap().to_str().unwrap().to_string();
    *prompt = format!("{}: {} % ", line_num, cwd);
}

//handles cmd lines that end with \, &&, |, ||
fn set_needs_continuation_prompt(prompt: &mut String, op: &str) {
    match op {
        "\\" => *prompt = String::from("> "),
        "&&" => *prompt = String::from("CmdAnd> "), 
        "||" => *prompt = String::from("CmdOr> "),
        "|" => *prompt = String::from("pipe> "),
        _ => *prompt = String::from("> "),
    }
}

//handles unclosed quoted strings, heredocs with no closing delimiters
fn set_expected_closer_prompt(prompt: &mut String, closer: &str) {
    match closer {
        "\'" | "`" => *prompt = String::from("quote> "),
        "\"" => *prompt = String::from("dquote> "),
        _ => *prompt = format!("missing {} for heredoc> ", closer),
    }
} 

fn exit_shell(rl: &mut DefaultEditor) {
    //for i in 0..rl.history().len() {
    //    let entry = rl.history().get(i, SearchDirection::Forward).unwrap().unwrap().entry;
    //    println!("{}: {}", i, entry);
    //}
    println!("exiting shell...");
    if rl.history_mut().save(Path::new(EDITOR_HISTORY)).is_err() {
        println!("Failed to save history");
    } else {
        println!("editory history saved to {}", EDITOR_HISTORY);
    }
    
    process::exit(0);
}
