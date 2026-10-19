# replace, replace_all, $1 $name ${name} $$, and a function as the replacement
let date = Regex.new '(?<year>\d{4})-(?<month>\d{2})'
let text = "from 2026-10 to 2027-01"
print date.replace(text, '$month/$year')
print date.replace_all(text, '${month}/${year}')
print Regex.new('(\d+)').replace_all("a1b22", '[$1]')
print Regex.new('x').replace("axb", '$$')
let computed = date.replace_all text do |m|
  "#{m.month}.#{m[1]}"
end
print computed
