use logos::{Logos, Lexer, SpannedIter };
use std::collections::VecDeque;
use std::ops::Range;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct LexerState { //re-initialize to new instance on every lex of cmd_buf
    //for heredocs
    delimiters: VecDeque<String>,
    heredocs: Vec<(usize, usize)>, //(doc_start, doc_end)

    pub syntax_err: Option<String>,
    pub group_closers: VecDeque<char>,
    pub expected_closer: Option<String>,
    pub continuation_for: Option<String>, //if cmd ends with &&, ||, |, or \, need to prompt user
}

impl LexerState {
    pub fn new() -> Self {
        Self {
            delimiters: VecDeque::new(),
            heredocs: Vec::new(),
            syntax_err: None,
            group_closers: VecDeque::new(),
            expected_closer: None,
            continuation_for: None,
        }
    }
}

#[derive(Logos, Debug, PartialEq, Clone, Serialize, Deserialize)]
#[logos(extras = LexerState)]
#[logos(skip r#"[ \t\f]+"#)]
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

    #[token(";")]
    Semicolon,

    #[token("&")]
    And,

    #[regex(r#"[`'"]"#, quote_handler)]
    Quote(String),

    #[token("(", bracket_callback)]
    LParen,

    #[token(")", bracket_callback)]
    RParen,

    #[token("\n", newline_handler)]
    Newline,
}

/*
 * NOTE: if a logos callback function returns a Option/Result/bool and 
 * the callback None/Err/false is returned, then the lex.next() call that triggers the callback will be Some(Err(_))
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
fn operator_callback(lex: &mut Lexer<Tkn>) -> Option<()> {
    let mut delim_lex = lex.clone().morph::<TargetDelim>();
    let operator = delim_lex.slice();
    //look ahead to see if the next token is a valid delimiter
    //does not advance lex iterator. i.e. if delim_lex finds valid delimiter,
    //it will be consumed as a Tkn::Word in the next lext.next() call
    match delim_lex.next() {
        Some(Ok(TargetDelim::Delim(_)) | Ok(TargetDelim::Quote(_))) => {
            delim_lex.extras.continuation_for = None;
        },
        Some(Ok(TargetDelim::Newline)) => {
            delim_lex.extras.continuation_for = Some(operator.to_string());
        },
        _ => { //invalid input following operator, like another shell operator, (), etc.
            delim_lex.extras.syntax_err = Some(format!("parse error near {}", delim_lex.span().end));
        },
    }

    lex.extras = delim_lex.extras; //match LexerStates
    Some(())
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

fn bracket_callback(lex: &mut Lexer<Tkn>) -> bool {
    let bracket = lex.slice();
    let mut success = false;
    match bracket {
        "(" => lex.extras.group_closers.push_front(')'),
        "[" => lex.extras.group_closers.push_front(']'),
        "{" => lex.extras.group_closers.push_front('}'),
        ")" | "]" | "}" => {
            if let Some(closer) = lex.extras.group_closers.front() {
                if bracket.chars().nth(0).unwrap() == *closer {
                    lex.extras.group_closers.pop_front();
                    success = true;
                } 
            } 
        }
        _ => (),
    }
    success
}

//returns a VecDeque of heredocs (if any) to be handed to the parser
fn newline_handler(lex: &mut Lexer<Tkn>) -> Option<()> {
    let mut heredoc_start = lex.span().end; //heredoc (if any) starts right after the newline
    let mut heredoc_end = lex.span().end;
    let mut heredoc_lex = lex.clone().morph::<HeredocTkn>();

    while let Some(delim) = heredoc_lex.extras.delimiters.pop_front() {
        let mut closed = false;
        //let mut heredoc_content = String::new();
        while let Some(res) = heredoc_lex.next() {
            match res {
                Ok(HeredocTkn::HeredocLine) => {
                    let line = heredoc_lex.slice();
                    if line.trim_end() == &delim {
                        closed = true;
                        break;
                    }
                    heredoc_end += line.len();
                    //heredoc_content.push_str(&line);
                },
                Err(e) => panic!("ERR: {:?}", e),
            }
        }
        if closed {
            heredoc_lex.extras.heredocs.push((heredoc_start, heredoc_end));
            heredoc_start = heredoc_end;
            //heredoc_lex.extras.heredocs.push(heredoc_content);
        } else { //we have to poll for more input from shell
            heredoc_lex.extras.expected_closer = Some(delim);
            *lex = heredoc_lex.morph();
            return None;        
        }
    }

    //set the span of Tkn lexer to match the whole quoted string content
    let num_read_bytes = lex.remainder().len() - heredoc_lex.remainder().len();
    lex.bump(num_read_bytes); 

    lex.extras = heredoc_lex.extras;
    Some(())
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(extras = LexerState)]
enum QuoteTkn {
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
enum TargetDelim { //for finding valid target after one of <, >, <<, or <<
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
enum HeredocTkn {
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

pub struct TknSpan {
    pub kind: Tkn,
    pub span: Range<usize>,
}

pub fn lex_cmd_buf<'a> (span_iter: &mut SpannedIter<'a, Tkn>, cmd_buf: &'a str) -> Option<(Vec<TknSpan>, VecDeque<&'a str>)> {
    let mut tkns: Vec<TknSpan> = Vec::new();
    //fresh borrow of span_iter so can get span_iter.extras later
    for (res, span) in &mut *span_iter {
        match res {
            Ok(tkn) => {
                match tkn {
                    Tkn::Newline => println!(),
                    Tkn::Quote(ref quote_content) => {
                        print!("tkn: \"{}\"; ", quote_content);
                    },
                    _ => print!("tkn: {}; ", &cmd_buf[span.start..span.end]),
                }
                tkns.push(TknSpan {kind: tkn, span});
            },
            Err(_) => {
                println!();
                return None;
            }
        }
    }

    if !span_iter.extras.group_closers.is_empty() { return None; } //unclosed paren, bracket, or curly

    //check if 2nd to last tkn is an invalid operator (last is newline)
    if let Some(tkn) = tkns.get(tkns.len()-2) { 
        if [Tkn::Semicolon, Tkn::Pipe, Tkn::CmdOr, Tkn::CmdAnd, ].contains(&tkn.kind) {
            return None;
        }
    }
    let mut heredocs = VecDeque::with_capacity(span_iter.extras.heredocs.len());
    for (doc_start, doc_end) in span_iter.extras.heredocs.iter() {
        let heredoc = &cmd_buf[*doc_start..*doc_end];
        heredocs.push_back(heredoc);
    }
    Some((tkns, heredocs))
}

pub fn get_token_at<'a>(tkn_span: &'a TknSpan, cmd_buf: &'a str) -> &'a str {
    if let Tkn::Quote(ref quote_content) = tkn_span.kind {
        return quote_content;
    }
    &cmd_buf[tkn_span.span.start..tkn_span.span.end]
}


