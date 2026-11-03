; Highlight query for Python, written for Europa on top of the
; queries/highlights.scm of tree-sitter-python 0.25.0.
;
; tree-sitter-highlight uses the last pattern that matches a node, so the
; general patterns come first and the specific ones after them.

; Keywords. Unlike the bundled query, this leaves out the Python 2 statements
; `print` and `exec`, takes the word operators as keywords, and adds `type`.
; `match`, `case` and `type` are anonymous nodes only where they act as
; keywords, so a variable of the same name stays plain.
[
  "and"
  "as"
  "assert"
  "async"
  "await"
  "break"
  "case"
  "class"
  "continue"
  "def"
  "del"
  "elif"
  "else"
  "except"
  "finally"
  "for"
  "from"
  "global"
  "if"
  "import"
  "in"
  "is"
  "is not"
  "lambda"
  "match"
  "nonlocal"
  "not"
  "not in"
  "or"
  "pass"
  "raise"
  "return"
  "try"
  "type"
  "while"
  "with"
  "yield"
] @keyword

; Literals. The escape comes after the string because it sits inside one and
; has to end up on top of it.
[
  (none)
  (true)
  (false)
] @constant.builtin

[
  (integer)
  (float)
] @number

(comment) @comment

(string) @string

(escape_sequence) @string.escape

; Calls of built-in functions, which are those of Python 3.12. They come after
; the keywords so that `type(x)` is a call and `type X = int` a keyword.
((call
  function: (identifier) @function.builtin)
 (#match?
   @function.builtin
   "^(abs|aiter|all|anext|any|ascii|bin|bool|breakpoint|bytearray|bytes|callable|chr|classmethod|compile|complex|delattr|dict|dir|divmod|enumerate|eval|exec|filter|float|format|frozenset|getattr|globals|hasattr|hash|help|hex|id|input|int|isinstance|issubclass|iter|len|list|locals|map|max|memoryview|min|next|object|oct|open|ord|pow|print|property|range|repr|reversed|round|set|setattr|slice|sorted|staticmethod|str|sum|super|tuple|type|vars|zip|__import__)$"))

; Names given at the definition of a function or a class, not their uses.
(function_definition
  name: (identifier) @function.definition)

(class_definition
  name: (identifier) @function.definition)

; Decorators, with the `@` and the name or the call of the decorator, but not
; the arguments of the call.
(decorator "@" @decorator)

(decorator
  (identifier) @decorator)

(decorator
  (attribute) @decorator)

(decorator
  (call
    function: (identifier) @decorator))

(decorator
  (call
    function: (attribute) @decorator))
