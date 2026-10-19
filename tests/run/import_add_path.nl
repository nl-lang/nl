# add_path appends a directory to the search path for files compiled after the call
add_path("import/extra")
let result = include("include/uses_extra.nl")
print result.doubled
