# A file runs once per state; every importer shares its top-level state
import { bump, current } from 'import/stateful'
import { bump_twice } from 'import/user'
bump()
print bump_twice()
print current()
