# Terminal half of the live demo: the TUI plays X from the keyboard while
# live.py plays O in the browser on the same game. make.sh exports TTT_BIN and
# TTT_PORT and runs this from docs/screenshots/<lang>/.
Output "tui.mp4"
Set Shell zsh
Set FontSize 24
Set Width 1200
Set Height 900
Set Padding 16
Set Theme "Builtin Dark"

Hide
Type '"$TTT_BIN" --ai off --port "$TTT_PORT"'
Enter
Sleep 3s
Show
Sleep 1500ms
Type "5"
Sleep 3s
Type "2"
Sleep 3s
Type "8"
Sleep 2s
Screenshot "tui.png"
Sleep 3s
Hide
Type "q"
Sleep 500ms
