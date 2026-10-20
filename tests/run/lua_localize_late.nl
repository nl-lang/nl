# a library field that a file imported later adds is read where it stands; Lua's own are read once, unless #!localize none
import { shout_all, rep_std } from 'import/late_reader'
import { rep_none } from 'import/late_none'
import { installed } from 'import/late_writer'
print installed
print shout_all("hey")
print rep_std("ab")
print rep_none("ab")
