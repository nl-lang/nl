# `on` with a qualified event name, `as 'name'`, `async`,
# block parameters and a return type.
on PlayerJoined do |player: Player|
end
on PlayerHit as 'armor' do |target, amount|
end
on PlayerJoined as 'reminder' async do |player|
end
on Game.Tick do
end
on E do |a: Number, b: String|: nil
end
h = on E do end
