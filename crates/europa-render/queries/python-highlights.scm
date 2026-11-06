; Highlight query for Python, written for Europa on top of the
; queries/highlights.scm of tree-sitter-python 0.25.0.
;
; When several patterns capture the same node, tree-sitter-highlight uses the
; last of them. Nested nodes are not in conflict: the innermost capture is on
; top wherever it lies, whatever the order of the patterns.

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

; Literals. An escape sits inside a string, so it is on top of the string.
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

; Parts of an f-string. The replacement field is Python again, so `@embedded`
; cancels the decoration of the string around it, and what only belongs to the
; f-string syntax is in one color of its own. These nodes sit inside the
; string, so they are on top of it.
(interpolation
  "{" @punctuation.special
  "}" @punctuation.special) @embedded

(interpolation
  "=" @punctuation.special)

(interpolation
  (type_conversion) @punctuation.special)

(format_specifier) @punctuation.special

(format_expression
  "{" @punctuation.special
  "}" @punctuation.special) @embedded

; Calls of built-in functions, which are those of Python 3.12. The `type` of
; `type(x)` is an identifier, while that of `type X = int` is the anonymous
; keyword node, so the two never meet.
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
; the arguments of the call. A decorator such as `@staticmethod(f)` calls a
; built-in, so the call pattern above captures the same identifier; these come
; after it so that the decorator wins.
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
