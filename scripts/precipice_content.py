"""Offline native animation/audio baking for the pinned Precipice Vive release."""
import collections
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import numpy as np


def bake_content(objects, world, active, transform, key, eye, material, staging):
    motions, node_motions, uv_nodes, flashes = [], {}, {}, []
    for o in objects:
        if o.type.name != 'MonoBehaviour':
            continue
        d = o.read(check_read=False)
        if not d.m_Script or not d.m_GameObject or not d.m_Enabled:
            continue
        t = transform(d.m_GameObject.read())
        if not active(t):
            continue
        name = d.m_Script.read().m_ClassName
        raw = o.get_raw_data()[32:]
        m = world(t)
        origin = (m[:3, 3]-eye)*[1, 1, -1]
        if name.startswith('CarMover') and len(raw) >= 12:
            values = struct.unpack('<'+'f'*(len(raw)//4), raw[:len(raw)//4*4])
            if name in ['CarMoverX', 'CarMoverXminus', 'CarMoverZ', 'CarMoverZminus']:
                start, finish, height, side, speed = values[:5]
                axis = 0 if 'X' in name else 2
                # Preserve the authored route axis/endpoints. Native playback
                # avoids the legacy double-local-translation/frame-rate quirks.
                velocity = np.zeros(3)
                velocity[axis] = math.copysign(abs(speed), finish-start)
                reset = np.array([start, height, side] if axis == 0 else [side, height, start])
                motion = {'kind': 'traffic', 'origin': origin.tolist(),
                          'velocity': (velocity*[1, 1, -1]).tolist(), 'axis': axis,
                          'start': float((reset-eye)[axis]*([1, 1, -1][axis])),
                          'finish': float((finish-eye[axis])*([1, 1, -1][axis])),
                          'reset': ((reset-eye)*[1, 1, -1]).tolist()}
            else:
                # Bake the original frame-dependent lift scripts at 60 Hz into
                # a compact position curve. Playback is independent of XR Hz.
                position = m[:3, 3].copy(); initial = position.copy(); speed = 0.; dt = 1/60
                keys = [[0., *([0., 0., 0.])]]
                for frame in range(1, 60*180+1):
                    if name == 'CarMoverY':
                        start, finish, xpos, zpos, acceleration, decelerate, dec_speed = values[:7]
                        if position[1] <= decelerate: speed += acceleration*dt
                        if position[1] >= decelerate: speed = max(0., speed-dec_speed*dt)
                        position += m[:3, 1]*speed
                        if position[0] <= finish: position = np.array([xpos, start, zpos]); speed = 0.
                    else:
                        acceleration, threshold, end = values[:3]
                        if position[1] >= threshold:
                            speed += acceleration*dt
                            position += m[:3, 2]*speed
                        if position[0] <= end: speed = 0.
                    if frame % 6 == 0:
                        keys.append([frame*dt, *((position-initial)*[1, 1, -1]).tolist()])
                    if frame > 120 and np.linalg.norm(position-initial) < 1e-7: break
                motion = {'kind': 'curve', 'origin': origin.tolist(), 'keys': keys}
            node_motions[key(t)] = len(motions); motions.append(motion)
        elif name == 'rotate_neon_01':
            speed, = struct.unpack_from('<f', raw)
            axis = m[:3, 1]*[1, 1, -1]
            node_motions[key(t)] = len(motions)
            motions.append({'kind': 'rotation', 'origin': origin.tolist(), 'velocity': axis.tolist(), 'speed': -speed})
        elif name == 'UVscroll':
            index, u, v = struct.unpack_from('<iff', raw)
            uv_nodes[key(t)] = (index, [u, v])
        elif name == 'flashingLight':
            # Authored ping-pong range, represented by a small emissive halo.
            values = struct.unpack('<'+'f'*(len(raw)//4), raw)
            flashes.append((t, values))

    def chain(t):
        result = []
        while t:
            if key(t) in node_motions: result.append(node_motions[key(t)])
            t = t.m_Father.read() if t.m_Father else None
        return tuple(reversed(result))

    particles, particle_data = [], []
    renderers = {key(o.read().m_GameObject.read()): o.read() for o in objects if o.type.name == 'ParticleSystemRenderer'}
    def scalar(x, default=0.): return float(x.get('scalar', default)) if isinstance(x, dict) else float(x or default)
    def curve(x):
        if not isinstance(x, dict): return float(x)
        values = x.get('maxCurve', {}).get('m_Curve', [])
        return scalar(x)*(values[0]['value'] if x.get('minMaxState') == 1 and values else 1.)
    for o in objects:
        if o.type.name != 'ParticleSystem': continue
        obj = o.read(); g = obj.m_GameObject.read(); t = transform(g)
        if not active(t): continue
        data = o.read_typetree(); init = data['InitialModule']; shape = data['ShapeModule']; emission = data['EmissionModule']
        r = renderers.get(key(g))
        if not data.get('playOnAwake') or not emission['enabled'] or r is None or not r.m_Enabled: continue
        mat = material(r.m_Materials[0]); texture, mode, cutoff, base, light, _, _ = mat
        life = max(.1, scalar(init['startLifetime'], 1)); rate = max(0, scalar(emission['rate']))
        count = min(int(init.get('maxNumParticles', 1000)), int(rate*life), 2000)
        rng = np.random.default_rng(o.path_id)
        m = world(t); rotation = m[:3, :3]/np.maximum(np.linalg.norm(m[:3, :3], axis=0), 1e-9)
        if shape['type'] == 5:
            spawn = (rng.random((count, 3))-.5)*[shape['boxX'], shape['boxY'], shape['boxZ']]
            spawn = spawn@m[:3, :3].T + m[:3, 3]
        else:
            a = rng.uniform(0, 2*math.pi, count); radius = np.sqrt(rng.random(count))*shape.get('radius', 1)
            spawn = np.column_stack([np.cos(a)*radius, np.sin(a)*radius, np.zeros(count)])@rotation.T+m[:3, 3]
        velocity = np.broadcast_to(rotation[:, 2]*scalar(init['startSpeed']), (count, 3)).copy()
        acceleration = np.broadcast_to([0., -9.81*scalar(init['gravityModifier']), 0.], (count, 3)).copy()
        vm = data.get('VelocityModule', {})
        if vm.get('enabled'):
            initial = np.array([curve(vm[k]) for k in ['x', 'y', 'z']])
            final = np.array([scalar(vm[k])*vm[k].get('maxCurve', {}).get('m_Curve', [{'value':1}])[-1]['value'] for k in ['x', 'y', 'z']])
            if not vm.get('inWorldSpace'): initial = rotation@initial; final = rotation@final
            velocity += initial; acceleration += (final-initial)/life
        size = scalar(init['startSize'], 1)
        # Stretch-billboard renderers encode elongated rain streaks; steam uses
        # ordinary camera-facing quads with a smooth lifetime envelope.
        rd = r.object_reader.read_typetree(); stretch = rd.get('m_RenderMode') == 1
        height = size*max(1., float(rd.get('m_LengthScale', 1))) if stretch else size
        sx = np.full(count, size); sy = np.full(count, height)
        if 'steam' in g.m_Name.lower(): sy *= 3; sx *= 3
        rows = np.column_stack([(spawn-eye)*[1, 1, -1], velocity*[1, 1, -1], acceleration*[1, 1, -1],
                                np.full(count, life), rng.uniform(0, life, count), sx, sy]).astype('<f4')
        first = sum(len(x) for x in particle_data); particle_data.append(rows)
        color = np.maximum(light+base[:3]*.3, .005)
        particles.append({'texture': texture, 'mode': max(1, mode), 'first': first, 'count': count,
                          'color': [*color.tolist(), float(base[3])], 'steam': 'steam' in g.m_Name.lower(), 'motions': list(chain(t))})
    flare = next((o for o in objects if o.type.name == 'Material' and o.read().m_Name == 'light_flare_01'), None)
    if flare is not None:
        texture = material(flare)[0]
        for t, values in flashes:
            light = next((ref.read() for _, ref in t.m_GameObject.read().m_Component if ref.type.name == 'Light'), None)
            if light is None: continue
            position = (world(t)[:3, 3]-eye)*[1, 1, -1]
            size = max(.08, min(light.m_Range, 2.)*.3)
            row = np.array([[*position, 0.,0.,0., 0.,0.,0., 100000.,0.,size,size]], dtype='<f4')
            first = sum(len(x) for x in particle_data); particle_data.append(row)
            color = np.array([light.m_Color.r,light.m_Color.g,light.m_Color.b])**2.2*light.m_Intensity
            particles.append({'texture':texture,'mode':2,'first':first,'count':1,'color':[*color.tolist(),1.],
                              'steam':False,'motions':list(chain(t)),'pulse':[max(.001,values[1]),values[2],values[3]]})
    if particle_data: (staging/'particles.bin').write_bytes(np.concatenate(particle_data).tobytes())

    sounds, audio_cache = [], {}
    for o in objects:
        if o.type.name != 'AudioSource': continue
        a = o.read(); g = a.m_GameObject.read(); t = transform(g)
        if not active(t) or not a.m_Enabled or not a.m_PlayOnAwake or a.Mute or not a.m_audioClip: continue
        clip = a.m_audioClip.read(); cache = (key(clip), round(a.m_Pitch, 4))
        if cache not in audio_cache:
            samples = clip.samples
            name, content = next(iter(samples.items()))
            source = staging/('source-'+str(len(audio_cache))+Path(name).suffix); source.write_bytes(content)
            target = 'audio-'+str(len(audio_cache))+'.opus'
            filters = []
            if a.m_Pitch < 0: filters.append('areverse')
            filters += [f'asetrate={clip.m_Frequency*abs(a.m_Pitch):.6f}', 'aresample=48000']
            ambient = 'bg' in clip.m_Name
            subprocess.run(['ffmpeg','-v','error','-y','-i',str(source),'-af',','.join(filters),'-ac','2' if ambient else '1',
                            '-ar','48000','-c:a','libopus','-b:a','48000' if ambient else '24000','-vbr','on',str(staging/target)], check=True)
            source.unlink(); audio_cache[cache] = target
        position = (world(t)[:3, 3]-eye)*[1, 1, -1]
        attached = g.m_Name in ['Camera (head)', 'Main Camera']
        sounds.append({'file': audio_cache[cache], 'gain': a.m_Volume, 'looping': bool(a.Loop),
                       'position': position.tolist(), 'attached': attached, 'motions': list(chain(t)),
                       'min_distance': a.MinDistance, 'max_distance': a.MaxDistance,
                       'spatial': 0. if attached else float(a.panLevelCustomCurve.m_Curve[0].value)})
    return motions, chain, uv_nodes, particles, sounds


def pack_meshes(batches, staging):
    """Exact indexing with half-float UV/lighting; positions retain float32 metres."""
    vertex_dtype = np.dtype([('position','<f4',3),('uv','<f2',2),('light','<f2',4),('emission','<f2',4)])
    assert vertex_dtype.itemsize == 32
    packed, draws, cursor = [], [], 0
    for (texture, mode, cutoff, motions, scroll), parts in sorted(batches.items(), key=lambda kv: kv[0][1]):
        rows = np.concatenate(parts); data = np.zeros(len(rows), dtype=vertex_dtype)
        data['position'] = rows[:,:3]; data['uv'] = rows[:,3:5]; data['light'][:,:3] = rows[:,5:8]
        data['light'][:,3] = rows[:,11]; data['emission'][:,:3] = rows[:,8:11]
        packed.append(data)
        draws.append({'texture': texture, 'mode': mode, 'cutoff': cutoff, 'motions': list(motions), 'uv_scroll': list(scroll),
                      'first': cursor, 'count': len(rows)})
        cursor += len(rows)
    all_vertices = np.concatenate(packed)
    unique, inverse = np.unique(all_vertices.view('V32'), return_inverse=True)
    (staging/'vertices.bin').write_bytes(unique.tobytes())
    (staging/'indices.bin').write_bytes(inverse.astype('<u4').tobytes())
    # Lossless PNG optimisation and content deduplication; small monochrome
    # artwork often reduces to a palette without reducing its resolution.
    from PIL import Image
    hashes, aliases = {}, {}
    for path in sorted(staging.glob('*.png')):
        image = Image.open(path).convert('RGBA')
        digest = hashlib.sha256(image.tobytes()+struct.pack('<II',*image.size)).hexdigest()
        if digest in hashes: aliases[path.name] = hashes[digest]; path.unlink(); continue
        hashes[digest] = path.name
        colors = image.getcolors(256)
        if colors is not None:
            palette = image.quantize(colors=max(2,len(colors)), method=Image.Quantize.FASTOCTREE)
            if palette.convert('RGBA').tobytes() == image.tobytes(): image = palette
        image.save(path,optimize=True,compress_level=9)
    return draws, len(unique), aliases


def compress_binaries(staging):
    """Lossless zstd storage; decoded buffers retain the exact GPU vertex data."""
    import ctypes
    zstd = ctypes.CDLL('libzstd.so.1')
    zstd.ZSTD_compressBound.argtypes = [ctypes.c_size_t]; zstd.ZSTD_compressBound.restype = ctypes.c_size_t
    zstd.ZSTD_compress.argtypes = [ctypes.c_void_p,ctypes.c_size_t,ctypes.c_void_p,ctypes.c_size_t,ctypes.c_int]
    zstd.ZSTD_compress.restype = ctypes.c_size_t
    zstd.ZSTD_isError.argtypes = [ctypes.c_size_t]; zstd.ZSTD_isError.restype = ctypes.c_uint
    for path in staging.glob('*.bin'):
        data = path.read_bytes(); target = ctypes.create_string_buffer(zstd.ZSTD_compressBound(len(data)))
        size = zstd.ZSTD_compress(target,len(target),data,len(data),12)
        if zstd.ZSTD_isError(size): raise ValueError('zstd mesh compression failed')
        path.with_suffix(path.suffix+'.zst').write_bytes(target.raw[:size]); path.unlink()
