use rustyline::{DefaultEditor, Result};
use rustyline::error::ReadlineError;
use std::process::Stdio;
use std::{env, process};
use std::path::{Path, PathBuf};
use shellwords::{split};

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

fn handle_line(l: String) -> Result<()> {
    let progs: Vec<&str> = l.split('|').collect(); 
    //only one program to execute
    if progs.len() == 1 {
        if let Some(args) = get_args(progs[0]) {
            match args[0].as_str() {
                "?" => println!("help:"),
                "exit" => process::exit(0),
                "pwd" => println!("{}", env::current_dir().unwrap().display()),
                "cd" => set_cwd(&args),
                _ => {
                    //will search $PATH if not absolute path
                    match process::Command::new(args[0].clone()).args(&args[1..]).status() {
                        Ok(_) => (),
                        Err(e) => println!("ERR: {}", e),
                    }
                },
            }
        }
        return Ok(())
    }
    //multiple programs, so set up pipes and fork processes
    let mut children: Vec<process::Child> = Vec::with_capacity(progs.len());
    let mut args: Vec<String>;
    for (i, prog) in progs.iter().enumerate() {
        match get_args(*prog) {
            Some(_args) => args = _args,
            None => continue,
        }
        let mut child = process::Command::new(args[0].clone());
        if i != progs.len() - 1 {
            child.stdout(Stdio::piped());
        }
        if i != 0 {
            let prev_prog = children.get_mut(i-1).unwrap();
            child.stdin(prev_prog.stdout.take().expect("Failed to open stdout"));
        }
        match child.args(&args[1..]).spawn() {
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
    Ok(())
}

fn get_args(prog: &str) -> Option<Vec<String>> {
    match split(prog) {
        Ok(args) => Some(args),
        Err(e) => {
            println!("ERR: {}", e);
            None
        }
    }
}

fn set_cwd(args: &Vec<String>) {
    if args.len() == 1 {
        let home = env::home_dir().expect("Couldn't find HOME directory");
        env::set_current_dir(&home).expect("ERR chdir");
    } else if args.len() == 2 {
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

fn main() -> Result<()> {
    let mut line_num = 0;
    let mut rl = DefaultEditor::new()?;

    loop {
        match rl.readline(&format!("{}: ", line_num)) {
            Ok(l) => {
                let _ = handle_line(l);
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
