# Server and client: a net message both ways.

if SERVER
  Util.add_network_string "nl_greet"
  Util.add_network_string "nl_ping"

  Net.receive "nl_ping" do |len: Number, ply: Player|
    Net.start "nl_greet"
    Net.write_string "pong after #{len} bits"
    Net.write_uint ply.user_id, 16
    Net.write_vector ply.get_pos
    Net.write_entity ply
    Net.send ply
  end

  fn greet_all(text: String)
    Net.start "nl_greet"
    Net.write_string text
    Net.write_uint 0, 16
    Net.write_vector vector_origin
    Net.write_entity NULL
    Net.broadcast
  end

  greet_all "hello"
end

if CLIENT
  Net.receive "nl_greet" do |len: Number|
    let text = Net.read_string()
    let id = Net.read_uint(16)
    let pos = Net.read_vector()
    let ent = Net.read_entity()
    print "#{text} (#{len}): #{id} at #{pos}"
    print "from #{ent.get_class}" if ent.is_valid
  end

  Net.start "nl_ping"
  Net.send_to_server
end
