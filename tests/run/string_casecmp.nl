# Case-insensitive comparison: fold, casecmp (-1, 0, 1), equal_ignoring_case, with Unicode folding
print "Héllo Wörld".fold
print "STRASSE".fold, "straße".fold
print "ΣΊΣΥΦΟΣ".fold

print "apple".casecmp("APPLE"), "apple".casecmp("Banana"), "cherry".casecmp("BANANA")
print "straße".casecmp("STRASSE"), "é".casecmp("É"), "é".casecmp("f")

print "Héllo".equal_ignoring_case("hÉLLO")
print "straße".equal_ignoring_case("STRASSE")
print "abc".equal_ignoring_case("abd")

# A list sorted without regard to case
let names = ["bob", "Émile", "alice", "Charlie", "ébène"]
Table.sort(names, |a, b| -> a.casecmp(b) < 0)
print Table.concat(names, " ")
