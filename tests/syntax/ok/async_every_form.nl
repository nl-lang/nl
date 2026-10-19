# `async fn` declares or, in an operand, is an anonymous async
# function; `async do` after a call or after `on Name` makes it async.
async fn load
end
f = async fn() end
g async do end
on E as 'h' async do end
