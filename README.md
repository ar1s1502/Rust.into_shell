TODO: 
  - arrow key support for history
  - signal capture
  - pseudo terminal (PTY)
  - tilde expansion logic, system user display
  - mutiline input parsing (e.g. copy-pasting multiline input)
  - line continuation support (the \ character at end of line)

features so far:
  - unix CLI tools supported via searching through $PATH variable
  - piping (e.g. cat Cargo.toml | head -n 10 | grep "bin")
  - basic redirection (<, >, >>)
  - heredoc support (<< operator)
    e.g. ```cat << EOF | grep "bin"
            bingchilling
            binturong
            EOF```
  - editor history with rustyline DefaultEditor. 

