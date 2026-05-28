TODO: 
  - pseudo terminal (PTY)
  - tilde expansion logic, system user display
  - tauri/react gui integration
  - ||, &&, and & support
  - autocomplete for shell commands

features so far:
  - unix CLI tools supported via searching through $PATH variable
  - piping (e.g. cat Cargo.toml | head -n 10 | grep "bin")
  - redirection (<, >, >>)
  - heredoc support (<< operator)
  - editor history with [Rustyline](https://docs.rs/rustyline/18.0.0/rustyline/) DefaultEditor, arrow key support
  - Fully custom Shell Lexer using the [Logos](https://docs.rs/logos/latest/logos/) crate

**USE**
if have cargo/rust, do `cargo r` from terminal within project directory. if not, wait till i make this a desktop app lol
Cool commands to try
 ```
     cat << EOF | grep "bin" >> tmp.txt
     bingchilling
     binturong
     EOF
     ```
     should append bingchilling\nbinturong to tmp.txt

     ```
     cat << one << two << three
     FIRST
     one
     SECOND
     two
     THIRD
     three
     ```
     should print FIRST\nSECOND\nTHIRD in terminal

