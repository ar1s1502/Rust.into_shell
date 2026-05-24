use rustyline::{DefaultEditor};
use rustyline::error::ReadlineError;
use std::process::Stdio;
use std::{env, process};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use shellwords::{split};
use anyhow::anyhow;

struct ChildPr { //a child process spawned by shell
    pub handle: process::Command,
    stdin: Option<Stdio>, 
    stdout: Option<Stdio>,
    pub prog_name: String,
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
        self.handle_redirect(); //apply <, >, <<, >> etc. 
        match self.handle.spawn() {
            Ok(c) => Ok(c),
            Err(e) => Err(anyhow!("{}", e)),
        }
    }

    pub fn status(&mut self) -> anyhow::Result<process::ExitStatus> {
        self.handle_redirect();
        match self.handle.status() {
            Ok(stat) => Ok(stat),
            Err(e) => Err(anyhow!("{}", e)),
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

fn handle_line(l: String) {
    let progs: Vec<&str> = l.split('|').collect(); 
    //only one program to execute
    if progs.len() == 1 {
        if let Ok(mut child_pr) = parse_program(progs[0]) {
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
        }
        return
    }
    //multiple programs, so set up pipes and fork processes
    let mut children: Vec<process::Child> = Vec::with_capacity(progs.len());
    let mut cur_child: ChildPr;
    for (i, prog) in progs.iter().enumerate() {
        match parse_program(*prog) {
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
            let prev_prog = children.get_mut(i-1).unwrap();
            cur_child.set_stdin(Stdio::from(prev_prog.stdout.take().expect("Failed to open stdout")));
        }
        match cur_child.spawn() {
            Ok(c) => children.push(c),
            Err(e) => println!("ERR: {}", e),
        }
    }

    children.iter_mut().map(|child| child.wait()).for_each(move |res| {
        match res {
            Ok(_) => (),
            Err(e) => println!("ERR: {}", e),
        }
    });
}

fn parse_program(prog: &str) -> anyhow::Result<ChildPr> {
    let tkns: Vec<String>;
    match split(prog) {
        Ok(t) => tkns = t,
        Err(e) => return Err(anyhow!("{}", e))
    }
    let mut stdin = None;
    let mut stdout = None;
    let mut args = Vec::new();
    let mut i = 0;
    while i < tkns.len() {
        match tkns[i].as_str() {
            ">" => {
                i += 1;
                //println!("tkns[{}]: {}", i, tkns[i]);
                match File::create(tkns[i].as_str()) {
                    Ok(f) => stdout = Some(Stdio::from(f)),
                    Err(e) => return Err(anyhow!("{}", e)),
                };
            },
            "<" => {
                i += 1;
                //println!("tkns[{}]: {}", i, tkns[i]);
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
            //TODO: "<<"
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

fn main() -> rustyline::Result<()> {
    let mut line_num = 0;
    let mut rl = DefaultEditor::new()?;

    loop {
        match rl.readline(&format!("{}: ", line_num)) {
            Ok(l) => {
                handle_line(l);
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
    Ok(())

}
