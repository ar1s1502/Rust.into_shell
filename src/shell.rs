use rustyline::{DefaultEditor};
use rustyline::error::ReadlineError;
use rustyline::history::{History, SearchDirection};
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

fn handle_cmd(cmd_buf: &str) {
    let (cmd, mut heredocs) = extract_heredocs(cmd_buf);
    let mut progs: Vec<&str> = cmd.split('|').collect(); 
    //only one program to execute
    if progs.len() == 1 {
        if progs[0].trim().is_empty() { return; }
        match parse_program(progs[0], &mut heredocs) {
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
    progs.retain(|&prog| !prog.trim().is_empty()); //remove all empty commands
                                                         //i.e. [cmd] | | [cmd] is ok
    for (i, prog) in progs.iter().enumerate() {
        if (*prog).trim().is_empty() { 
            continue; 
        }
        match parse_program(*prog, &mut heredocs) {
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

fn parse_program(prog: &str, heredocs: &mut VecDeque<String>) -> anyhow::Result<ChildPr> {
    let tkns: Vec<String>;
    match split(prog) {
        Ok(t) => tkns = t,
        Err(e) => return Err(anyhow!("{}", e)),
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
            "<<" => {
                i += 1;
                if tkns.get(i).is_none() {
                    return Err(anyhow!("please specify a delimiter"));
                }
                if let Some(heredoc) = heredocs.pop_front() {
                    heredoc_content = Some(heredoc);
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
        heredoc_content: heredoc_content,
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

fn validate_heredoc(cmd: &str) -> bool {
    //if contains "<< (*)" then check if another line has the delimiter
    //   if not then switch to heredoc continuation
    //if has << but no delimiter, then ok
    if let Some(idx) = cmd.find("<<") {
        let after_arrow = cmd[idx + 2..].trim_start();
        let delim = after_arrow.split(|c: char| c.is_whitespace() || c == '|' || c == ';')
            .next()
            .unwrap_or("");

        if delim.is_empty() {
            return true; // No delimiter typed yet, keep polling
        }
        let lines = cmd.get(idx..).unwrap_or("").split('\n');
        for l in lines.skip(1) {
            if l.trim_end() == delim {
                return true 
            }
        }
        return false //must switch to heredoc> continuation
    }
    true
}

fn extract_heredocs(cmd: &str) -> (String, VecDeque<String>) {
    let mut heredocs = VecDeque::new();
    let mut clean_cmd = String::new();
    let mut lines = cmd.lines();
    while let Some(line) = lines.next() {
        clean_cmd.push_str(line);
        clean_cmd.push('\n');
        if let Some(idx) = line.find("<<") {
            match line.get((idx+2)..) {
                Some(text) => {
                    let delim = text.trim_start().split(|c: char| c.is_whitespace() || c == '|' || c == ';')
                        .next()
                        .unwrap_or("");
                    if !delim.is_empty() {
                        let mut payload = String::new();
                        while let Some(heredoc_content) = lines.next() {
                            if heredoc_content.trim_end() == delim { break; }
                            payload.push_str(heredoc_content);
                            payload.push('\n');
                        }
                        heredocs.push_back(payload);
                    }
                },
                None => continue,
            }
        }
    }
    (clean_cmd, heredocs)
}

fn validate_input(cmd_: &str) -> Option<String> {
    let cmd = cmd_.trim();
    let mut fix_prompt = String::new();
    match split(cmd) {
        Ok(tkns) => {
            if let Some(last_tkn) = tkns.last() {
                match last_tkn.as_str() {
                    "|" => {
                        fix_prompt.push_str("pipe"); 
                    },
                    "||" => {
                        fix_prompt.push_str("cmdor"); 
                    },
                    "&&" => {
                        fix_prompt.push_str("cmdand"); 
                    },
                    _ => (),
                }
            }
            if !validate_heredoc(cmd) {
                fix_prompt.push_str("heredoc");
            }
        },
        Err(_) => {
            if cmd.ends_with('\\') {
                //backslash continuation
                fix_prompt.push_str("backslash");
            } else {
                //missing closing " or '
                fix_prompt.push_str("dquote");
            }
        }
    }
    if fix_prompt.is_empty() {
        None
    } else {
        fix_prompt.push_str("> ");
        Some(fix_prompt)
    }
}

fn main() -> rustyline::Result<()> {
    let mut line_num = 0;
    let mut rl = DefaultEditor::new()?;
    if rl.history_mut().load(Path::new(EDITOR_HISTORY)).is_err() {
        println!("Failed to load shell command history");
    }
    let mut cmd_buf = String::new();
    let mut cwd = env::current_dir().unwrap().file_name().unwrap().to_str().unwrap().to_string();
    let mut prompt = format!("{}: {} % ", line_num, cwd);

    loop {
        match rl.readline(&prompt) {
            Ok(input) => {
                if input.is_empty() {continue;}
                if input.trim() == "exit" { exit_shell(&mut rl); };
                cmd_buf.push_str(&input);
                if let Some(fix_prompt) = validate_input(&cmd_buf) {
                    prompt = fix_prompt;
                    cmd_buf.push('\n');
                } else {
                    cwd = env::current_dir().unwrap().file_name().unwrap().to_str().unwrap().to_string();
                    prompt = format!("{}: {} % ", line_num, cwd);
                    handle_cmd(&cmd_buf); //parse and execute
                    let _ = rl.add_history_entry(&cmd_buf);
                    cmd_buf.clear();
                }
            },
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                cwd = env::current_dir().unwrap().file_name().unwrap().to_str().unwrap().to_string();
                prompt = format!("{}: {} % ", line_num, cwd);
                cmd_buf.clear();
            },
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                cwd = env::current_dir().unwrap().file_name().unwrap().to_str().unwrap().to_string();
                prompt = format!("{}: {} % ", line_num, cwd);
                cmd_buf.clear();
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

fn exit_shell(rl: &mut DefaultEditor) {
    for i in 0..rl.history().len() {
        let entry = rl.history().get(i, SearchDirection::Forward).unwrap().unwrap().entry;
        println!("{}: {}", i, entry);
    }
    if rl.history_mut().save(Path::new(EDITOR_HISTORY)).is_err() {
        println!("Failed to save history");
    }
    
    process::exit(0);
}
