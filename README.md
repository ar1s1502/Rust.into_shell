Shell program in pure Rust. 

TODO: 
  - pseudo terminal (PTY)
  - tilde expansion logic, system user display
  - tauri/react gui integration
  - ||, &&, and & support
  - autocomplete for shell commands

features so far:
  - unix CLI tools supported via searching through $PATH variable. 
  - piping (e.g. cat Cargo.toml | head -n 10 | grep "bin") via process spawning.
  - redirection (<, >, >>)
  - heredoc support (<< operator)
  - editor history with [Rustyline](https://docs.rs/rustyline/18.0.0/rustyline/) DefaultEditor, arrow key support
  - Fully custom, state-aware, error-aware shell Lexer using the [Logos](https://docs.rs/logos/latest/logos/) crate. "state" is managed through Logos as well. Different Logos enums = different shell lexer state, as how input is lexed into tokens changes depending on which enum the lexer is using to match into tokens. for example the normal lexer, which matches words and shell operators, will switch into a different lexer upon encountering a quote token like \` ' or " (for example a `>` is interpreted literally if within quotes, while it is the stdout redirect operator otherwise). If you were to try to execute a command that has a grammar/semantics error (e.g. unclosed quote, heredoc or redirect operator without a valid delimiter), the lexer will recognize this and will prompt the read loop for more input. TLDR; it behaves like a real Bash/zsh shell in your terminal. If you type `echo "hello` you're going to see a new prompt line asking you to close the double quote, and then it will execute

**USE**  
if have cargo/rust, do `cargo r` from terminal within project directory. if not, wait till i make this a desktop app lol.
This shell has not been tested on Windows platforms; only on Mac. if you are a Windows user, probably going to need Git Bash so that the unix CLI tools can still be found in system $PATH. 

Cool commands to try  

 ```
cat << EOF | grep "bin" >> tmp.txt
 bingchilling
 binturong
 EOF
```
 should append
 ```
 bingchilling
 binturong
```
 to tmp.txt

     ```
     cat << one << two << three
     FIRST
     one
     SECOND
     two
     THIRD
     three
     ```
should print
```
FIRST
SECOND
THIRD
```

