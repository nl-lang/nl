# A trailing block after the first name, a `.name`, a
# `:name` or a call step; it is the last argument of that call.
:each do |x| x end
:t.each do |x| x end
:t:each do |x| x end
:t.map(f) do |x| x end
