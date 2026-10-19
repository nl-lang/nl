# Heredoc strings and their flags
let a = "A"
let plain = """
multi
line #{a}
"""
print plain
print plain.len
let dedented = """-
  first
    indented
  last #{a}
"""
print dedented
print dedented.len
let raw = """=
no #{a} and no \n escape
"""
print raw
let raw_dedented = """-=
    kept #{a}
    as written
"""
print raw_dedented
print "end"
