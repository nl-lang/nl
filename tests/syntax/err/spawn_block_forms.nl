# `spawn do` takes no parameters and no `async`.
a = spawn do |x| end
b = spawn async do end
