# The constructors rt.new makes for struct literals (runtime/nl_20_struct.lua,
# CONSTRUCTORS) do what its loops do: the same instances, the same errors,
# the defaults and the field types evaluated in the same order, also while
# a field type is incomplete, after it changed, and for the structs that
# get no constructor. Each line ends with what evaluated the defaults.
include("lib/new_shapes.lua")
