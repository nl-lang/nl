# Prefixes, suffixes, line endings, substrings: include, count, remove, insert, is_empty, is_blank
let s = "héllo wörld"
print s.delete_prefix("hé"), s.delete_prefix("x"), s.delete_suffix("rld"), s.delete_suffix("")
print "x".ensure_prefix("/"), "/x".ensure_prefix("/"), "a".ensure_suffix(";"), "a;".ensure_suffix(";")

# chomp: one line ending, every trailing newline with "", or a given suffix
print "[" <> "abc\r\n".chomp <> "]", "[" <> "abc\n\n".chomp <> "]", "[" <> "abc\r".chomp <> "]"
print "[" <> "abc\n\r\n".chomp("") <> "]", "[" <> "abc.txt".chomp(".txt") <> "]", "[" <> "abc".chomp("x") <> "]"

# chop: the last character, or a "\r\n"
print "[" <> "wörld".chop <> "]", "[" <> "ab\r\n".chop <> "]", "[" <> "".chop <> "]"

print s.include("wö"), s.contains("xyz"), s.include("")

# count: occurrences of a substring, without overlap; "" is found between characters
print "banana".count("an"), "aaaa".count("aa"), s.count("ö"), s.count("")

print s.remove("l"), "a.b.c".remove("."), "abc".remove("")

# insert: before the character at a position; a negative one inserts after
print "abcd".insert(1, "X"), "abcd".insert(3, "X"), "abcd".insert(-1, "X"), "abcd".insert(-2, "X")
print "héllo".insert(3, "·"), "abc".insert(99, "!")

print "".is_empty, " ".is_empty, "".is_blank, " \t\n".is_blank, " x ".is_blank
print "[" <> "  héllo \t  wörld \n".squish <> "]"
