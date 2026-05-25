use rustyline::{DefaultEditor};
use rustyline::error::ReadlineError;
use rustyline::history::{History, SearchDirection};
use std::process::Stdio;
use std::{env, process};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::io::Write;
use shellwords::{split};
use anyhow::anyhow;

struct ChildPr { //a child process spawned by shell
    pub handle: process::Command,
    stdin: Option<Stdio>, 
    stdout: Option<Stdio>,
    pub prog_name: String,
    pub heredoc_content: Option<String>, //handling << operator
    pub appended_content: Option<String>, //handling dquote, pipe, etc. 
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
                if let Some(buf) = self.heredoc_content.clone() {
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
                if let Some(buf) = self.heredoc_content.clone() {
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

fn handle_line(l: &str, rl: &mut DefaultEditor) {
    let progs: Vec<&str> = l.split('|').collect(); 
    //only one program to execute
    if progs.len() == 1 {
        match parse_program(progs[0], rl) {
            Ok(mut child_pr) =>{
                match child_pr.prog_name.as_str() {
                    "?" => println!("help:"),
                    "exit" => process::exit(0),
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

                //add full program to editor history 
                let mut entry = l.to_string();
                let mut child_prs = vec![child_pr];
                make_entry(&mut child_prs, &mut entry);
                let _ = rl.add_history_entry(&entry);
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
        match parse_program(*prog, rl) {
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

    let mut entry = l.to_string();
    make_entry(&mut child_prs, &mut entry);
    let _ = rl.add_history_entry(&entry);
}

fn parse_program(prog: &str, rl: &mut DefaultEditor) -> anyhow::Result<ChildPr> {
    let tkns: Vec<String>;
    let mut appended_content = None;
    match split(prog) {
        Ok(t) => tkns = t,
        Err(_) => {
            let mut buffer = String::from("\n");
            loop {
                match rl.readline("dquote> ") {
                    Ok(l) => {
                        buffer.push_str(&l);
                        buffer.push('\n');
                        if l.trim().ends_with("\"") { break; }
                    },
                    Err(ReadlineError::Interrupted) => return Err(anyhow!("CTRL-C: user interrupted")),
                    Err(ReadlineError::Eof) => return Err(anyhow!("CTRL-D: user interrupted")),
                    Err(e) => return Err(anyhow!("{}", e)),
                }
            }
            let fixed = format!("{}{}", prog, buffer); 
            tkns = split(&fixed).unwrap();
            buffer.replace_range(..1, ""); //skip newline char in buffer
            appended_content = Some(buffer);
        }
    }
    let mut stdin = None;
    let mut stdout = None;
    let mut heredoc_content = None;
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
            "<<" => { //heredoc
                i += 1;
                let delimiter: &String;
                match tkns.get(i) {
                    Some(tkn) => delimiter = tkn,
                    None => return Err(anyhow!("please specify a delimiter")),
                };
                let mut buffer = String::new();
                loop {
                    match rl.readline("heredoc> ") {
                        Ok(l) => {
                            if l.trim() == delimiter { break; }
                            buffer.push_str(&l);
                            buffer.push('\n');
                        },
                        Err(ReadlineError::Interrupted) => return Err(anyhow!("CTRL-C: user interrupted")),
                        Err(ReadlineError::Eof) => return Err(anyhow!("CTRL-D: user interrupted")),
                        Err(e) => return Err(anyhow!("{}", e)),
                    }
                }
                heredoc_content = Some(buffer);
                stdin = Some(Stdio::piped());
            },
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
        heredoc_content: heredoc_content,
        appended_content: appended_content,
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

fn make_entry(child_prs: &mut Vec<ChildPr>, entry: &mut String) {
    entry.push('\n');
    for child_pr in child_prs {
        if let Some(buf) = child_pr.heredoc_content.take() {
            entry.push_str(&buf);
        } else if let Some(buf) = child_pr.appended_content.take() {
            entry.push_str(&buf);
        }
    }
}

fn main() -> rustyline::Result<()> {
    let mut line_num = 0;
    let mut rl = DefaultEditor::new()?;

    loop {
        let cwd = env::current_dir().unwrap().file_name().unwrap().to_str().unwrap().to_string();
        match rl.readline(&format!("{}: {} % ", line_num, cwd)) {
            Ok(l) => {
                if l.is_empty() {continue;}
                handle_line(&l, &mut rl);
            },
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break
            },
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break
            },
            Err(err) => {
                println!("error: {:?}", err);
                break
            },
        }
        line_num += 1;
    }
    for i in 0..rl.history().len() {
        let entry = rl.history().get(i, SearchDirection::Forward).unwrap().unwrap().entry;
        println!("{}: {}", i, entry);
    }
    Ok(())

}
