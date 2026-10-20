# Case styles of identifiers and phrases, and upcase_first/downcase_first
print "UsableItem".snake_case
print "HTTPServer".snake_case
print "usable item-name".snake_case
print "version2Beta".snake_case
print "usable_item".camel_case
print "UsableItem".camel_case
print "usable_item".pascal_case
print "HTTPServer".pascal_case
print "UsableItem".kebab_case
print "man from the boondocks".title_case
print "x-men: the last stand".title_case

# Unicode letters have case too
print "ÉtéChaud".snake_case
print "été_chaud".pascal_case
print "héllo wörld".title_case

# parameterize: lowercased, other characters as one separator, none at the ends
print "Héllo Wörld!".parameterize
print "  Donald E. Knuth  ".parameterize
print "Donald E. Knuth".parameterize(sep: "_")
print "foo - bar".parameterize
print "a b".parameterize(sep: "")

# Only the first character changes, unlike capitalize
print "usableItem".upcase_first
print "UsableItem".downcase_first
print "éLAN".upcase_first
print "ÉLAN".downcase_first
print "usableItem".capitalize
print "[" <> "".upcase_first <> "]"
