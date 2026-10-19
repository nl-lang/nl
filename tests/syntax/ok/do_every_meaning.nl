# `do` at the start of a statement is a scope block; after a call
# it attaches a trailing block; after `spawn`, `on Name` and `async` it is part
# of those forms.
do
end
f do end
t = spawn do end
on E do end
f async do end
