# Rejected by the analyzer.
# A file that imports this one may define `fn Table.second`, but this file
# runs first and cannot know it, so the read would be a key of the list.
pub fn use_first(xs: [Number]) xs.first end

# expect-error: no function of Table is named `second`, and a list has no keys of that kind
pub fn use_second(xs: [Number]) xs.second end
