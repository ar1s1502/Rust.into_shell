mod lexer;
mod parser;
mod executor;

use lexer::{Tkn, TknSpan, lex_cmd_buf, LexerState};
use executor::{execute_cmd_buf, execute_ast};
use logos::{Logos, };
use rustyline::{DefaultEditor};
use rustyline::error::ReadlineError;
use rustyline::history::{History, SearchDirection};
use std::{env, process};
//use std::path::{Path, PathBuf};
use std::io::Write;
use std::collections::VecDeque;
use std::cell::{RefCell};
use anyhow::anyhow;
use std::time::Duration;
use std::thread;

const HISTORY_PATH: &str = "rust_shell_history.txt"; 
pub const AS_SUBSHELL: &str = "--as-subshell";

thread_local! {
    pub static CMD_HISTORY: RefCell<Vec<String>> = RefCell::new(Vec::new()); 
}

/* 
OSC Escape sequence's data so that frontend typescript can differentiate between shell outputs
OSC 133 syntax: \x1b]133;${data}\x07
see https://contour-terminal.org/vt-extensions/osc-133-shell-integration/ 
*/
const PROMPT_START: &str = "A";
//custom OSC data key PROMPT_CONTINUE for PROMPT_START
const PROMPT_END: &str = "B";
const CMD_OUTPUT_START: &str = "C";
const CMD_END: &str = "D";

fn print_cmd<'a> (tkns: &'a [TknSpan], heredocs: &VecDeque<&'a str>) {
    for tkn in tkns.iter() {
        println!("{:?}, {:?}", tkn.kind, tkn.span);
    }
    println!("heredocs: {:?}", heredocs);
}

fn main() -> rustyline::Result<()> {
    //check if this process is running as a subshell
    //TODO: add cryptography to ensure that the subshell really spawned by shell
    if let Ok(serialized_ast) = std::env::var(AS_SUBSHELL) {
        let parsed_ast = serde_json::from_str(&serialized_ast).unwrap_or_else(|_| {
            process::exit(1);
        });
        match execute_ast(parsed_ast) {
            Ok(exit_code) => process::exit(exit_code),
            Err(_) => process::exit(1),
        }
    }
    //else, this is running in the foreground, do REPL 
    send_osc133(CMD_OUTPUT_START);
    let mut rl = DefaultEditor::new()?;
    if let Err(e) = load_history(&mut rl) {
        println!("{}", e);
    } else {
        for i in 0..rl.history().len() {
            CMD_HISTORY.with_borrow_mut(|h| 
                h.push(rl.history().get(i, SearchDirection::Forward).unwrap().unwrap()
                    .entry.to_string())
            )
        }
    }
    let mut cmd_buf = String::new();
    let mut prompt = String::new();
    send_osc133(CMD_END);
    set_normal_prompt(&mut prompt,);
    loop {
        //sleep fixes a weird race condition where sometimes the promptline prints before the output
        //on the cat << A | cat << B | cat << C test case
        thread::sleep(Duration::from_millis(2)); 
        match rl.readline(&prompt) {
            Ok(input) => {
                if input.trim().is_empty() { continue; }
                if input.trim().to_lowercase() == "exit" { exit_shell(&mut rl); };
                cmd_buf.push_str(&input);
                cmd_buf.push('\n'); //add back newline that readline stripped after user hit Enter
                let lex_state = LexerState::new();
                let mut lex = Tkn::lexer_with_extras(&cmd_buf, lex_state).spanned();
                match lex_cmd_buf(&mut lex, &cmd_buf) {
                    Some((tkns, heredocs)) => {
                        send_osc133(PROMPT_END);
                        send_osc133(CMD_OUTPUT_START);
                        print_cmd(&tkns, &heredocs);
                        if let Err(e) = execute_cmd_buf(&cmd_buf, &tkns, heredocs) {
                            println!("ERR: {}", e);
                        }
                        send_osc133(CMD_END);
                        set_normal_prompt(&mut prompt,);
                        //Add to history
                        let _ = rl.add_history_entry(cmd_buf.trim());
                        CMD_HISTORY.with_borrow_mut(|h| h.push(cmd_buf.trim().to_string()));
                        cmd_buf.clear();
                    },
                    None => {
                        if let Some(ref err) = lex.extras.syntax_err {
                            send_osc133(PROMPT_END);
                            //syntax errs get highest priority b/c they're unrecoverable
                            send_osc133(CMD_OUTPUT_START);
                            println!("Syntax ERR: {}", err);
                            send_osc133(CMD_END);
                            set_normal_prompt(&mut prompt,);
                            let _  = rl.add_history_entry(cmd_buf.trim());
                            cmd_buf.clear();
                        } else if let Some(ref closer) = lex.extras.expected_closer {
                            set_expected_closer_prompt(&mut prompt, closer);
                        } else if let Some(ref op) = lex.extras.continuation_for {
                            set_needs_continuation_prompt(&mut prompt, op);
                        } else if let Some(ref bracket) = lex.extras.bracket_closers.front() {
                            set_expected_closer_prompt(&mut prompt, &bracket.to_string());
                        }
                    }
                }
            },
            Err(ReadlineError::Interrupted) => {
                send_osc133(PROMPT_END);
                println!("CTRL-C");
                set_normal_prompt(&mut prompt,);
                cmd_buf.clear();
            },
            Err(ReadlineError::Eof) => {
                send_osc133(PROMPT_END);
                println!("CTRL-D");
                set_normal_prompt(&mut prompt,);
                cmd_buf.clear();
            },
            Err(err) => {
                send_osc133(PROMPT_END);
                println!("ERR: {:?}", err);
            },
        }
    }
}

fn set_normal_prompt(prompt: &mut String) { 
    let cwd = env::current_dir().unwrap().file_name().unwrap().to_str().unwrap().to_string();
    let username = whoami::account().unwrap_or("<unknown>".to_string());
    let devicename = whoami::devicename().unwrap_or("<unknown>".to_string()).replace(" ","-");
    *prompt = format!("[rust_shell] {}@{}: {} % ", username, devicename, cwd);
    send_osc133(PROMPT_START);
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
    send_osc133(&format!("{};{}", PROMPT_START, prompt));
}

//handles unclosed quoted strings, heredocs with no closing delimiters
fn set_expected_closer_prompt(prompt: &mut String, closer: &str) {
    match closer {
        "\'" => *prompt = String::from("quote> "),
        "`" => *prompt = String::from("bquote> "),
        "\"" => *prompt = String::from("dquote> "),
        ")" => *prompt = String::from("subsh> "),
        _ => *prompt = format!("missing {} for heredoc> ", closer),
    }
    send_osc133(&format!("{};{}", PROMPT_START, prompt));
} 

fn send_osc133(data: &str) {
    print!("\x1b]133;{}\x07", data);
    //flush because stdout might buffer until newline
    let _ = std::io::stdout().flush();
}

fn save_history(rl: &mut DefaultEditor) -> anyhow::Result<String> {
    if let Some(path) = env::home_dir() {
        let save_path = path.join(HISTORY_PATH);
        if rl.history_mut().save(save_path.as_path()).is_err() {
            return Err(anyhow!("Failed to save history"));
        } else {
            return Ok(format!("{}", save_path.display()));
        }
    } else { 
        return Err(anyhow!("Failed to save history"));
    }
}

fn load_history(rl: &mut DefaultEditor) -> anyhow::Result<()> {
    if let Some(path) = env::home_dir() {
        let save_path = path.join(HISTORY_PATH);
        if rl.history_mut().load(save_path.as_path()).is_err() {
            return Err(anyhow!("Failed to load history"));
        } else {
            return Ok(());
        }
    } else { 
        return Err(anyhow!("Failed to load history"));
    }
}

fn exit_shell(rl: &mut DefaultEditor) {
    send_osc133(CMD_OUTPUT_START);
    println!("exiting shell...");
    match save_history(rl) {
        Ok(save_path) => println!("shell command history saved to {}", save_path),
        Err(e) => println!("ERR: {}", e),
    }
    process::exit(0);
}
