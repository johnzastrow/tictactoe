# Query bar: field and value autocomplete, errors underlined as you type
Output "05-query.gif"
Source "../tapes/_setup.tape"
Sleep 800ms
Ctrl+F
Sleep 300ms
Type "status:active health:"
Sleep 1500ms
Screenshot "raw/tui-05-query.png"
Sleep 1500ms
Ctrl+U
Escape
Sleep 500ms
