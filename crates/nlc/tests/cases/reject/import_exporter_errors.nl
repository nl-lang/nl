# Importing from a file that exports a name twice
# fails at the import.
# expect-error: does not compile
import { f } from 'lib/export_twice'
print f
