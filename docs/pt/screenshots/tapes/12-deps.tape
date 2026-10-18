# Dependency chips: pick tasks of the same project by number or title
Output "12-deps.gif"
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
Enter
Sleep 400ms
Type "sprint"
Sleep 1500ms
Screenshot "raw/tui-12-deps.png"
Sleep 1500ms
Escape
Escape
Sleep 500ms
