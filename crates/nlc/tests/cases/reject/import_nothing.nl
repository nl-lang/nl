# Every import names what it binds; none runs a
# file without binding a name.
# expect-error: this import binds no name
import { } from 'lib/kinds'
