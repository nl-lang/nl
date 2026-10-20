# Helper for import_table_methods: functions added to Table, which the
# files that import this one call as methods of typed tables.
fn Table.second
  self[2]
end

fn Table.total_by(key: String): Number
  self.map(row -> row[key]).sum
end

pub const loaded = true
