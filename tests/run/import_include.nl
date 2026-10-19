# include runs a file by its path on every call, .lua and .nl alike
let first = include("include/counter.lua")
let second = include("include/counter.lua")
print first
print second
let exports = include("include/exports.nl")
print exports.value
let again = include("include/exports.nl")
print again.value
