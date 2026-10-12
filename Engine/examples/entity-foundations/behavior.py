from rustic import rustic, Tags, Groups, Entity, Components, run

def on_start():
    e = rustic.entity_id()
    Tags.add(e, 'enemy')
    Groups.create('enemies')
    Groups.add('enemies', e)
    assert Entity.is_valid(e)
    assert Components.has(e, 'rustic.transform')
    assert len(Tags.find_all('enemy')) == 1
    rustic.log('info', 'entity foundations ready')
    Entity.destroy_after(e, 1)

run(globals())
