Shell program in pure Rust. 

TODO: 
  - pseudo terminal (PTY)
  - tilde expansion logic, system user display
  - tauri/react gui integration
  - ||, &&, and & support
  - autocomplete for shell commands

features so far:
  - unix CLI tools supported via searching through $PATH variable. 
  - piping (e.g. cat Cargo.toml | head -n 10 | grep "bin") via process spawning. Because using Rust's built in [std::process] library, this should work on Windows as well. 
  - redirection (<, >, >>)
  - heredoc support (<< operator)
  - editor history with [Rustyline](https://docs.rs/rustyline/18.0.0/rustyline/) DefaultEditor, arrow key support
  - Fully custom, state-aware, error-aware shell Lexer using the [Logos](https://docs.rs/logos/latest/logos/) crate. will pretty much behave exactly like Bash/zsh shells. "state" is managed through Logos as well. Different Logos enums = different shell lexer state; for example the normal lexer, which matches words and shell operators, will switch into a different lexer upon encountering a quote token like \` ' or " (for example a `>` is interpreted literally if within quotes, while it is the stdout redirect operator otherwise)

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

