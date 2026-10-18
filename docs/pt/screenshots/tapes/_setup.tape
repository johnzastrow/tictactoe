# Shared settings for every control tape (run from docs/pt/screenshots/<lang>/ by make.sh).
# make.sh exports PT_BIN, PT_SERVER and PT_CONFIG_DIR and serves a throwaway
# database seeded with the demo password typed below.
Set Shell zsh
Set FontSize 26
Set Width 1640
Set Height 960
Set Padding 16
Set Theme "Builtin Dark"
Set TypingSpeed 120ms
Set Framerate 24

Hide
Type '"$PT_BIN" tui --server "$PT_SERVER" --user alice'
Enter
Sleep 1500ms
Type "demo-pass-123"
Ctrl+S
Sleep 2500ms
Down
Show
