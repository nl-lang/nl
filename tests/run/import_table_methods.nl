# A `fn Table.name` of an imported file is a method of typed tables in the importer
import { loaded } from 'import/table_ext'

let xs: [Number] = [4, 5, 6]
print loaded, xs.second, xs.map(x -> x * 10).second
let rows: [Table] = [{n: 1}, {n: 2}]
print rows.total_by("n"), Table.second(rows).n
let m: {[String]: [Number]} = {a: [1, 2]}
print m["a"].second
