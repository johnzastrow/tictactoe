# Query bar: field and value autocomplete, errors underlined as you type
Output "tui-05-query.gif"
Output "raw/tui-05-query.mp4"
Source "../tapes/_setup.tape"
Sleep 800ms
# open: the control opens here (compose4.py syncs the 2x2 clips on it)
Ctrl+F
Sleep 300ms
Type "status:active health:"
Sleep 1500ms
Screenshot "raw/tui-05-query.png"
Sleep 1500ms
Ctrl+U
Escape
Sleep 500ms
