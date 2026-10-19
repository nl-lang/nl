# In block parameters a top-level `|` closes the list,
# so a short function as a default ends there too; parentheses hold a `|`.
f do |g = x -> x + 1| g end
f do |g = (x -> x | 1)| g end
