## The functions of tests/gmod/decl/gmod_min.d.nl under the names the
## programs of tests/gmod use, for their gmod-client bundles, which have
## gmod's declarations (decl/glua.d.nl) built in: these add no types.
## Realm: shared. https://wiki.facepunch.com/gmod/Global.Player
declare fn player_by_id(userid: Number): Player = :Player
## Realm: shared. https://wiki.facepunch.com/gmod/Global.Entity
declare fn entity_by_index(index: Number): Entity = :Entity
