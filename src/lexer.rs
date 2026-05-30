use logos::{Logos, Lexer, };
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct LexerState { //re-initialize to new instance on every lex of cmd_buf
    //for heredocs
    pub delimiters: VecDeque<String>,

    pub expected_closer: Option<String>,
    pub syntax_err: Option<String>,
    pub continuation_for: Option<String>, //if cmd ends with &&, ||, |, or \, need to prompt user
}

impl LexerState {
    pub fn new() -> Self {
        Self {
            delimiters: VecDeque::new(),
            expected_closer: None,
            syntax_err: None,
            continuation_for: None,
        }
    }
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(extras = LexerState)]
pub enum Tkn {
    #[regex(r#"[^ `"'\\\t\f\n|&;<>(){}]+"#)]
    Word,

    #[token("<", redirect_callback)]
    RedirectIn,

    #[token(">", redirect_callback)]
    RedirectOut,

    #[token(">>", redirect_callback)]
    RedirectAppend,

    #[token("<<", heredoc_callback)]
    Heredoc,

    #[token("|", operator_callback)] //handle pipe syntax in ./shell.rs
    Pipe,

    #[token("\\", operator_callback)]
    Backslash,

    #[token("&&", operator_callback)]
    CmdAnd,

    #[token("||", operator_callback)]
    CmdOr,

    #[token("&")]
    And,

    #[regex(r#"[ \t\f]+"#)]
    Whitespace,

    #[regex(r#"[`'"]"#, quote_handler)]
    Quote(String),

    #[token("\n", newline_handler)]
    Newline((VecDeque<String>, String)), //(Vec of heredocs, extra commands) if any
}

/*
 * NOTE: if a logos callback function returns a Option/Result and 
 * the callback None/Err is returned, then the lex.next() call that triggers the callback will be Some(Err(_))
 */

fn redirect_callback(lex: &mut Lexer<Tkn>) -> bool {
    let mut delim_lex = lex.clone().morph::<TargetDelim>();
    let operator = delim_lex.slice();
    let mut success = false;
    //look ahead to see if the next token is a valid filename
    //does not advance lex iterator. i.e. if delim_lex finds valid filename,
    //it will be consumed as a Tkn::Word in the next lext.next() call
    match delim_lex.next() {
        Some(Ok(TargetDelim::Delim(_)) | Ok(TargetDelim::Quote(_))) => {
            //found a valid filename
            success = true;
        },
        _ => {
            delim_lex.extras.syntax_err = Some(format!("not a valid delimiter for {}", operator));
        }
    }

    lex.extras = delim_lex.extras; //match LexerStates
    success
}

//handles |, ||, &&, and \
fn operator_callback(lex: &mut Lexer<Tkn>) -> bool {
    let mut delim_lex = lex.clone().morph::<TargetDelim>();
    let operator = delim_lex.slice();
    let mut success = false;
    //look ahead to see if the next token is a valid heredoc delimiter
    //does not advance lex iterator. i.e. if delim_lex finds valid delimiter,
    //it will be consumed as a Tkn::Word in the next lext.next() call
    match delim_lex.next() {
        Some(Ok(TargetDelim::Delim(_)) | Ok(TargetDelim::Quote(_))) => {
            delim_lex.extras.continuation_for = None;
            success = true;
        },
        Some(Ok(TargetDelim::Newline)) => {
            delim_lex.extras.continuation_for = Some(operator.to_string());
            success = true; //don't throw an error here because we want to prompt for continuation
        },
        _ => { //invalid input following operator, like another shell operator, (), etc.
            delim_lex.extras.syntax_err = Some(format!("parse error near {}", delim_lex.span().end));
        },

    }

    lex.extras = delim_lex.extras; //match LexerStates
    success
}

fn heredoc_callback(lex: &mut Lexer<Tkn>) -> bool {
    let mut delim_lex = lex.clone().morph::<TargetDelim>();
    let mut success = false;
    //look ahead to see if the next token is a valid heredoc delimiter
    //does not advance lex iterator. i.e. if delim_lex finds valid delimiter,
    //it will be consumed as a Tkn::Word in the next lext.next() call
    match delim_lex.next() {
        Some(Ok(TargetDelim::Delim(delim)) | Ok(TargetDelim::Quote(delim))) => {
            delim_lex.extras.delimiters.push_back(delim);
            success = true;
        },
        _ => {
            delim_lex.extras.syntax_err = Some("not a valid delimiter for <<".to_string());
        },

    }
    lex.extras = delim_lex.extras; //match LexerStates
    success                                   
}

//returns a VecDeque of heredocs (if any) to be handed to the parser
fn newline_handler(lex: &mut Lexer<Tkn>) -> Option<(VecDeque<String>, String)> {
    let mut heredoc_lex = lex.clone().morph::<HeredocTkn>();
    let mut heredocs = VecDeque::new();

    while let Some(delim) = heredoc_lex.extras.delimiters.pop_front() {
        let mut closed = false;
        let mut heredoc_content = String::new();
        while let Some(res) = heredoc_lex.next() {
            match res {
                Ok(HeredocTkn::HeredocLine) => {
                    let line = heredoc_lex.slice();
                    if line.trim_end() == &delim {
                        closed = true;
                        break;
                    }
                    heredoc_content.push_str(&line);
                },
                Err(e) => panic!("ERR: {:?}", e),
            }
        }
        if closed {
            heredocs.push_back(heredoc_content);
        } else { //we have to poll for more input from shell
            heredoc_lex.extras.expected_closer = Some(delim);
            *lex = heredoc_lex.morph();
            return None;        
        }
    }

    *lex = heredoc_lex.morph();
    
    let mut cmd_continuation = String::new(); //handles any more commands on a separate input line
    if lex.extras.continuation_for.is_some() {
        while let Some(res) = lex.next() {
            match res {
                //recursively search for more commands
                //newline_handler is called again if Tkn::NewLine found
                Ok(Tkn::Newline((_, cmds))) => {
                    cmd_continuation.push_str(lex.slice());
                    cmd_continuation.push_str(&cmds);
                    break;
                },
                Ok(Tkn::Word | Tkn::Quote(_)) => {
                    lex.extras.continuation_for = None;
                    cmd_continuation.push_str(lex.slice());
                }
                Ok(_) => cmd_continuation.push_str(lex.slice()),
                Err(_) => cmd_continuation.push_str(lex.slice()),
            }
        }
    }
    if lex.extras.continuation_for.is_some() { 
        //we didn't find valid commands following the operator
        return None; 
    }
    //ready to pass to parser
    Some((heredocs, cmd_continuation))
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(extras = LexerState)]
pub enum QuoteTkn {
    #[regex(r#"['"`]"#)] // Match any potential closer
    PotentialCloser,
    
    #[token("\\")]
    Escape,

    #[regex(r#"[^'"`\\]+"#, |lex| lex.slice().to_string())]
    //stop matching Text at a backslash, cuz bacsklash in quotes must escape next char
    Text(String),
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(extras = LexerState)]
#[logos(skip r"[ \t\f]+")] // Ignore this regex pattern between token
pub enum TargetDelim { //for finding valid target after one of <, >, <<, or <<
    // A valid delimiter is 1 or more characters that are NOT 
    // whitespace or shell operators.

    #[regex(r#"['"`]"#, quote_handler)]
    Quote(String),

    #[token("\n")]
    Newline,

    #[regex(r#"[^ `"'\t\n\f|&;<>(){}]+"#, |lex| lex.slice().to_string())]
    Delim(String),
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(extras = LexerState)]
pub enum HeredocTkn {
    //match any number of characters, ended with a newline
    #[regex(r#"[^\n]*\n"#, allow_greedy = true)]
    HeredocLine,
}

fn quote_handler<'a, T>(lex: &mut Lexer<'a, T>) -> Option<String> 
where T: Logos<'a, Extras = LexerState, Source = str> + Clone {
    assert!(lex.extras.expected_closer.is_none());
    let mut quote_lex = lex.clone().morph::<QuoteTkn>();
    //closing quote must match the opening quote
    quote_lex.extras.expected_closer = Some(quote_lex.slice().to_string());
    let mut content = String::new();
    while let Some(res) = quote_lex.next() {
        match res {
            Ok(QuoteTkn::PotentialCloser) => {
                let quote = quote_lex.slice();
                if Some(quote.to_string()) == quote_lex.extras.expected_closer {
                    quote_lex.extras.expected_closer = None;
                    break;
                }
                content.push_str(quote);
            },
            Ok(QuoteTkn::Text(text)) => {
                content.push_str(&text);
            },
            Ok(QuoteTkn::Escape) => {
                if Some("\'".to_string()) == quote_lex.extras.expected_closer {
                    content.push('\\'); //backslash doesn't escape in single quotes
                } else {
                    if let Some(_) = quote_lex.next() {
                        content.push_str(quote_lex.slice());
                    }
                }
            }
            Err(e) => panic!("ERR: {:?}", e),
        }
    } 
    
    //set the span of Tkn lexer to match the whole quoted string content
    let num_read_bytes = lex.remainder().len() - quote_lex.remainder().len();
    lex.bump(num_read_bytes); 

    lex.extras = quote_lex.extras; //sync states
    if lex.extras.expected_closer.is_none() { Some(content) } else { None }
}
