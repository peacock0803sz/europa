#!/usr/bin/env bash
# Classify corpus notebooks into load / reject reasons / review.
# Independent from the Rust implementation: uses jq and raw-byte checks.
set -euo pipefail

CORPUS_DIR="${EUROPA_CORPUS_DIR:-target/europa-corpus}"
TESTS_DIR="crates/europa-core/tests"
EXPECTED="$TESTS_DIR/corpus-expected.tsv"
OVERRIDES="$TESTS_DIR/corpus-overrides.tsv"

is_integer_literal() {
  # Returns 0 when the raw literal for a key looks like a plain integer.
  # Arguments: file key
  local file="$1"
  local key="$2"
  local literal
  literal=$(grep -o -E "\"$key\"[[:space:]]*:[[:space:]]*-?[0-9][0-9A-Za-z+._-]*" "$file" | head -n 1 || true)
  if [ -z "$literal" ]; then
    return 1
  fi
  local value
  value=$(printf '%s' "$literal" | sed -E 's/.*:[[:space:]]*//')
  case "$value" in
    *.*|*e*|*E*|+*|0[0-9]*)
      return 1
      ;;
  esac
  return 0
}

has_float_like_number() {
  # Detect float-like literals or very long integers that jq cannot tell apart.
  local file="$1"
  if grep -q -E '"(nbformat|nbformat_minor|execution_count)"[[:space:]]*:[[:space:]]*-?[0-9][0-9]*[.eE]' "$file"; then
    return 0
  fi
  if grep -q -E '"(nbformat|nbformat_minor|execution_count)"[[:space:]]*:[[:space:]]*-?[0-9]{16,}' "$file"; then
    return 0
  fi
  return 1
}

max_depth() {
  local file="$1"
  jq --stream -e '
    [inputs | .[0] | length] | max // 0
  ' "$file" 2>/dev/null || echo 0
}

has_duplicate_keys() {
  local file="$1"
  local dups
  dups=$(jq -c --stream 'select(length == 2) | .[0] | @json' "$file" 2>/dev/null | sort | uniq -d | head -n 1 || true)
  if [ -n "$dups" ]; then
    return 0
  fi
  return 1
}

has_lone_surrogate() {
  # Best-effort raw scan for unpaired \uD800-\uDFFF escapes.
  local file="$1"
  local seqs
  seqs=$(grep -o -E '\\u[dD][0-9a-fA-F]{4}' "$file" || true)
  if [ -z "$seqs" ]; then
    return 1
  fi
  local prev_high=0
  while IFS= read -r seq; do
    local hex
    hex=$(printf '%s' "$seq" | sed -E 's/\\u[dD]//I' | tr 'A-F' 'a-f')
    local code=$((16#$hex))
    if [ "$code" -ge 55296 ] && [ "$code" -le 56319 ]; then
      if [ "$prev_high" -eq 1 ]; then
        return 0
      fi
      prev_high=1
    elif [ "$code" -ge 56320 ] && [ "$code" -le 57343 ]; then
      if [ "$prev_high" -eq 0 ]; then
        return 0
      fi
      prev_high=0
    fi
  done <<< "$seqs"
  if [ "$prev_high" -eq 1 ]; then
    return 0
  fi
  return 1
}

classify_one() {
  local file="$1"
  local rel="$2"

  # (a) raw-byte checks before JSON parsing.
  if [ "$(head -c 3 "$file" | od -An -tx1 | tr -d ' \n')" = "efbbbf" ]; then
    printf '%s\ta\n' "$rel"
    return
  fi
  if ! iconv -f UTF-8 -t UTF-8 "$file" >/dev/null 2>&1; then
    printf '%s\ta\n' "$rel"
    return
  fi
  if has_lone_surrogate "$file"; then
    printf '%s\ta\n' "$rel"
    return
  fi
  if has_duplicate_keys "$file"; then
    printf '%s\ta\n' "$rel"
    return
  fi

  # Malformed JSON is (a).
  if ! jq -e . "$file" >/dev/null 2>&1; then
    printf '%s\ta\n' "$rel"
    return
  fi

  # Boundary depths and numeric spellings need human review.
  local depth
  depth=$(max_depth "$file")
  if [ "$depth" -ge 120 ] && [ "$depth" -le 136 ]; then
    printf '%s\treview\n' "$rel"
    return
  fi
  if has_float_like_number "$file"; then
    printf '%s\treview\n' "$rel"
    return
  fi
  if [ "$depth" -ge 128 ]; then
    printf '%s\ta\n' "$rel"
    return
  fi

  # Root shape comes before (b) and (c).
  if [ "$(jq -r 'type' "$file")" != "object" ]; then
    printf '%s\td\n' "$rel"
    return
  fi

  # (b) nbformat must be integer 4.
  if ! jq -e 'has("nbformat")' "$file" >/dev/null; then
    printf '%s\tb\n' "$rel"
    return
  fi
  local nb_type
  nb_type=$(jq -r '.nbformat | type' "$file")
  if [ "$nb_type" != "number" ]; then
    printf '%s\tb\n' "$rel"
    return
  fi
  if ! jq -e '.nbformat == 4 and (.nbformat | floor) == .nbformat' "$file" >/dev/null; then
    printf '%s\tb\n' "$rel"
    return
  fi
  if ! is_integer_literal "$file" "nbformat"; then
    printf '%s\treview\n' "$rel"
    return
  fi

  # (c) nbformat_minor must be a u32 non-negative integer.
  if ! jq -e 'has("nbformat_minor")' "$file" >/dev/null; then
    printf '%s\tc\n' "$rel"
    return
  fi
  if ! jq -e '(.nbformat_minor | type) == "number" and ((.nbformat_minor | floor) == .nbformat_minor) and .nbformat_minor >= 0 and .nbformat_minor <= 4294967295' "$file" >/dev/null; then
    printf '%s\tc\n' "$rel"
    return
  fi
  if ! is_integer_literal "$file" "nbformat_minor"; then
    printf '%s\treview\n' "$rel"
    return
  fi

  # (d) remaining structural checks in document order.
  if ! jq -e '(.metadata | type) == "object" and (.cells | type) == "array"' "$file" >/dev/null; then
    printf '%s\td\n' "$rel"
    return
  fi

  local verdict
  verdict=$(jq -r '
    def is_str_or_str_array: type == "string" or (type == "array" and all(.[]?; type == "string"));
    def check_output($cell_idx; $out_idx; $o):
      if ($o | type) != "object" then "d"
      elif ($o | has("output_type") | not) or ((($o.output_type? // null) | type) != "string") then "d"
      elif ["stream","display_data","execute_result","error"] | index($o.output_type?) | not then "load"
      elif $o.output_type? == "stream" then
        if ((($o.name? // null) | type) != "string") or ((($o.text? // null) | is_str_or_str_array) | not) then "d" else "load" end
      elif $o.output_type? == "display_data" then
        if ((($o.data? // null) | type) != "object") or ((($o.metadata? // null) | type) != "object") then "d" else "load" end
      elif $o.output_type? == "execute_result" then
        if ((($o.data? // null) | type) != "object") or ((($o.metadata? // null) | type) != "object") then "d"
        elif ($o | has("execution_count") | not) then "d"
        elif $o.execution_count? == null then "load"
        elif ((($o.execution_count? // null) | type) != "number") or ((($o.execution_count? // 0) | floor) != $o.execution_count?) then "d"
        elif $o.execution_count? < -9223372036854775808 or $o.execution_count? > 9223372036854775807 then "d"
        else "load" end
      elif $o.output_type? == "error" then
        if ((($o.ename? // null) | type) != "string") or ((($o.evalue? // null) | type) != "string") then "d"
        elif ((($o.traceback? // null) | type) != "array") or (([($o.traceback? // [])[]? | type] | all(. == "string")) | not) then "d"
        else "load" end
      else "load" end;
    def check_cell($idx; $c):
      if ($c | type) != "object" then "d"
      elif ($c | has("cell_type") | not) or ((($c.cell_type? // null) | type) != "string") then "d"
      elif ["code","markdown","raw"] | index($c.cell_type?) | not then "load"
      elif ((($c.source? // null) | is_str_or_str_array) | not) then "d"
      elif ((($c.metadata? // null) | type) != "object") then "d"
      elif $c.cell_type? != "code" then "load"
      elif ((($c.outputs? // null) | type) != "array") then "d"
      elif ($c | has("execution_count") | not) then "d"
      elif $c.execution_count? != null and (((($c.execution_count? // null) | type) != "number") or ((($c.execution_count? // 0) | floor) != $c.execution_count?)) then "d"
      elif $c.execution_count? != null and ($c.execution_count? < -9223372036854775808 or $c.execution_count? > 9223372036854775807) then "d"
      else ($c as $cell | [range(0; (($cell.outputs? // []) | length)) | check_output($idx; .; $cell.outputs[.])] | if index("d") then "d" else "load" end)
      end;
    (. as $root | ([range(0; ((($root.cells? // []) | length))) | check_cell(.; $root.cells[.])] | if index("d") then "d" else "load" end))
  ' "$file")
  if [ "$verdict" = "d" ]; then
    printf '%s\td\n' "$rel"
  else
    printf '%s\tload\n' "$rel"
  fi
}

main() {
  if [ ! -d "$CORPUS_DIR" ]; then
    echo "corpus directory not found: $CORPUS_DIR" >&2
    echo "run bash scripts/fetch-corpus.sh first" >&2
    exit 1
  fi

  declare -A overrides=()
  if [ -f "$OVERRIDES" ]; then
    while IFS=$'\t' read -r path verdict _rest; do
      case "$path" in
        ""|"#"*) continue ;;
      esac
      if [ -n "$path" ] && [ -n "$verdict" ]; then
        overrides["$path"]="$verdict"
      fi
    done < "$OVERRIDES"
  fi

  tmp=$(mktemp)
  trap 'rm -f "$tmp"' EXIT

  while IFS= read -r -d '' file; do
    rel=${file#"$CORPUS_DIR"/}
    if [ -n "${overrides["$rel"]:-}" ]; then
      printf '%s\t%s\n' "$rel" "${overrides["$rel"]}" >> "$tmp"
    else
      classify_one "$file" "$rel" >> "$tmp"
    fi
  done < <(find "$CORPUS_DIR" -type f -name '*.ipynb' -print0 | sort -z)

  sort "$tmp" > "$EXPECTED"
  echo "wrote $EXPECTED"
}

main "$@"
