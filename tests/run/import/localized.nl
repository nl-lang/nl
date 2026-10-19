# Helper for lua_path_localized.nl: a file that adds a function to the
# `string` library when it runs, and a global it
# keeps changing.
global LocalizedHits = 0

:string.shout = fn(s) "#{s.upcase}!" end
:string.counts = {imported: 1}

pub fn hit
  LocalizedHits += 1
  LocalizedHits
end
