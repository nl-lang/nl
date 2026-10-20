# fn on a declared host type adds a method to the metatable gmod has for the class, for NL and Lua code
fn Player.greeting(word: String): String
  "#{word}, #{self.nick}"
end

fn Entity.summary: String
  "#{self.get_class} #{self.ent_index}"
end

let bob = player_by_id(1)
print bob.greeting("hello")
print bob.summary
print entity_by_index(10).summary

# A value of no known type finds the method at run time.
let ann: any = player_by_id(2)
print ann.greeting("hi")

# Lua code calls it with `:`, on the class's own objects.
print :Player(1):greeting("hey")
print :Entity(11):summary()
