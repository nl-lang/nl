# Helper for import_prim_methods: imports prim_ops, and uses its operators.
import { loaded } from 'prim_ops'

pub fn joined(a: String, b: String): String
  a + b
end

pub const ready = loaded
