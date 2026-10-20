# Helper for import_prim_methods: methods and operators on String and
# Number, defined once for the whole state when this file runs.
fn String.+(right: String): String
  self <> " + " <> right
end

fn String.*(n: Number): String
  self.rep(n)
end

fn String.shout: String
  self.upcase <> "!"
end

fn Number.*(right: String): String
  right.rep(self)
end

pub const loaded = true
