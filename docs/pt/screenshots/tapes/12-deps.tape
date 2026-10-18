# Dependency chips: pick tasks of the same project by number or title
Output "tui-12-deps.gif"
Output "raw/tui-12-deps.mp4"
Source "../tapes/_setup.tape"
Sleep 800ms
Ctrl+K
Type "tasks"
Sleep 500ms
Enter
Sleep 800ms
Down 2
Right 12
Sleep 400ms
# open: the control opens here (compose4.py syncs the 2x2 clips on it)
Enter
Sleep 400ms
Type "sprint"
Sleep 1500ms
Screenshot "raw/tui-12-deps.png"
Sleep 1500ms
Escape
Escape
Sleep 500ms
