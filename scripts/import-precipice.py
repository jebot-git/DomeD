#!/usr/bin/env python3
"""Import DEEPFIELD's original LA2097 Vive scene for a local DomeD installation.

Requires UnityPy==1.25.4, numpy and Pillow (see --help). Does not execute the
Windows player or redistribute its assets. The original archive is hash pinned.
"""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import tempfile
import zipfile

ARCHIVE_SHA256 = '3dd6adef06c032cd195434a745790fcb2a589537f8ae330057c099fd3c5b4aeb'
SOURCE = 'https://deepfield.itch.io/la2097-precipice'


def convert(archive, output):
    import numpy as np
    import UnityPy
    from PIL import Image
    from UnityPy.helpers.MeshHelper import MeshHandler
    from UnityPy.export.MeshRendererExporter import get_mesh

    if hashlib.sha256(archive.read_bytes()).hexdigest() != ARCHIVE_SHA256:
        raise ValueError('Expected the original LA2097 v0.9.9.4 Vive archive')
    if output.exists():
        raise ValueError(f'Output already exists: {output}; use a new directory')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='precipice-') as temp:
        with zipfile.ZipFile(archive) as z:
            # Load only serialized data and texture resources, never executables.
            for info in z.infolist():
                name = Path(info.filename)
                if name.name in ['level0', 'globalgamemanagers', 'globalgamemanagers.assets',
                                  'resources.assets', 'resources.assets.resS',
                                  'sharedassets0.assets', 'sharedassets0.assets.resS', 'sharedassets0.resource',
                                  'unity default resources', 'unity_builtin_extra']:
                    with z.open(info) as src:
                        Path(temp, name.name).write_bytes(src.read())
        env = UnityPy.load(temp)
        objects = list(env.objects)
        transforms = {}
        for o in objects:
            if o.type.name == 'Transform':
                t = o.read()
                transforms[(o.assets_file.name, t.m_GameObject.path_id)] = t

        def key(obj):
            return (obj.object_reader.assets_file.name, obj.object_reader.path_id)

        def xyz(v):
            return np.array([v.x, v.y, v.z], dtype=np.float64)

        def rotation(q):
            x, y, z, w = q.x, q.y, q.z, q.w
            return np.array([[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
                             [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
                             [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]])

        worlds, actives = {}, {}

        def world(t):
            k = key(t)
            if k not in worlds:
                m = np.eye(4)
                m[:3, :3] = rotation(t.m_LocalRotation) @ np.diag(xyz(t.m_LocalScale))
                m[:3, 3] = xyz(t.m_LocalPosition)
                if t.m_Father:
                    m = world(t.m_Father.read()) @ m
                worlds[k] = m
            return worlds[k]

        def active(t):
            k = key(t)
            if k not in actives:
                actives[k] = bool(t.m_GameObject.read().m_IsActive) and (not t.m_Father or active(t.m_Father.read()))
            return actives[k]

        def transform(g):
            return transforms[(g.object_reader.assets_file.name, g.object_reader.path_id)]

        rig = next(t for t in transforms.values() if t.m_GameObject.read().m_Name == '[CameraRig]')
        # The rig is a floor origin. Centre the scene at a 1.4 m seated eye,
        # matching DomeD's default centre; do not scale the city or its stereo.
        eye = world(rig)[:3, 3] + [0, 1.4, 0]
        lights = []
        for o in objects:
            if o.type.name != 'Light':
                continue
            light = o.read()
            t = transform(light.m_GameObject.read())
            if not light.m_Enabled or not active(t):
                continue
            m = world(t)
            lights.append((light.m_Type, m[:3, 3], m[:3, 2],
                           np.array([light.m_Color.r, light.m_Color.g, light.m_Color.b])**2.2 * light.m_Intensity,
                           light.m_Range, np.cos(np.radians(light.m_SpotAngle / 2))))
        settings = next(o.read() for o in objects if o.type.name == 'RenderSettings')
        batches = collections.defaultdict(list)
        textures, materials, meshes = {}, {}, {}
        counts = collections.Counter()
        staging = Path(temp, 'output')
        staging.mkdir()
        Image.new('RGBA', (1, 1), (255, 255, 255, 255)).save(staging / 'white.png')

        def color(c):
            return np.array([c.r, c.g, c.b, c.a])

        def material(ptr):
            mat = ptr.read()
            k = key(mat)
            if k in materials:
                return materials[k]
            p = mat.m_SavedProperties
            colors = {k.name: color(v) for k, v in p.m_Colors}
            floats = {k.name: v for k, v in p.m_Floats}
            tex = {k.name: v for k, v in p.m_TexEnvs}
            shader = mat.m_Shader.read().m_Name.lower()
            additive = 'add' in shader
            transparent = additive or 'transparent' in shader or floats.get('_Mode', 0) >= 2
            cutout = 'cutout' in shader or floats.get('_Mode', 0) == 1
            base = colors.get('_Color', np.ones(4)).copy()
            emission = colors.get('_EmissionColor', np.zeros(4))[:3]
            if additive or 'particle' in shader:
                base = colors.get('_TintColor', base).copy()
                emission = base[:3] * 2
                base[:3] = 0
            path = 'white.png'
            uv_scale, uv_offset = np.ones(2), np.zeros(2)
            main = tex.get('_MainTex')
            if main:
                uv_scale = [main.m_Scale.x, main.m_Scale.y]
                uv_offset = [main.m_Offset.x, main.m_Offset.y]
                if main.m_Texture:
                    image = main.m_Texture.read()
                    tk = key(image)
                    if tk not in textures:
                        filename = f'texture-{len(textures)}.png'
                        # UnityPy returns top-left PNGs. Vertices retain Unity UVs;
                        # the native shader reverses V when sampling.
                        image.image.convert('RGBA').save(staging / filename)
                        textures[tk] = filename
                    path = textures[tk]
            # Material/ambient inspector colours are authored in Unity gamma space.
            base[:3] = np.maximum(base[:3], 0)**2.2
            emission = np.maximum(emission, 0)**2.2
            mode = 2 if additive else 1 if transparent else 0
            result = (path, mode, float(floats.get('_Cutoff', .5)) if cutout else 0., base, emission, uv_scale, uv_offset)
            materials[k] = result
            return result

        from precipice_content import bake_content, pack_meshes, compress_binaries
        motions, motion_chain, uv_nodes, particles, sounds = bake_content(
            objects, world, active, transform, key, eye, material, staging)

        for o in objects:
            if o.type.name != 'MeshRenderer':
                continue
            r = o.read()
            t = transform(r.m_GameObject.read())
            if not r.m_Enabled or not active(t) or r.m_GameObject.read().m_Name.lower() == 'logo':
                continue
            mesh = get_mesh(r)
            if mesh is None:
                continue
            mk = key(mesh)
            if mk not in meshes:
                handler = MeshHandler(mesh)
                handler.process()
                meshes[mk] = (handler, list(handler.get_triangles()))
            h, triangles = meshes[mk]
            subsets = list(r.m_SubsetIndices or [])
            combined = bool(subsets) or mesh.m_Name.startswith('Combined Mesh')
            # Unity's static batches already contain world-space vertices. Each
            # renderer owns only its subset, never the entire combined mesh.
            selected = subsets if subsets else list(range(len(triangles)))
            m = np.eye(4) if combined else world(t)
            normal_matrix = np.linalg.inv(m[:3, :3]).T
            for material_index, subset in enumerate(selected):
                if not r.m_Materials:
                    continue
                ptr = r.m_Materials[min(material_index, len(r.m_Materials)-1)]
                if not ptr:
                    continue
                path, mode, cutoff, base, emission, scale, offset = material(ptr)
                ids = np.array(triangles[subset], dtype=np.int64).reshape(-1)
                if not len(ids):
                    continue
                # Deduplicate while calculating vertex lighting, then expand to
                # triangle lists once, batching by texture and blend mode.
                unique, inverse = np.unique(ids, return_inverse=True)
                v = np.array(h.m_Vertices)[unique, :3] @ m[:3, :3].T + m[:3, 3]
                if h.m_Normals:
                    n = np.array(h.m_Normals)[unique, :3] @ normal_matrix.T
                else:
                    vertices = np.array(h.m_Vertices)[:, :3]
                    normals = np.zeros_like(vertices)
                    all_triangles = np.concatenate([np.array(ts, dtype=np.int64).reshape(-1, 3) for ts in triangles])
                    face = np.cross(vertices[all_triangles[:, 1]]-vertices[all_triangles[:, 0]], vertices[all_triangles[:, 2]]-vertices[all_triangles[:, 0]])
                    for corner in range(3):
                        np.add.at(normals, all_triangles[:, corner], face)
                    n = normals[unique] @ normal_matrix.T
                n /= np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-8)
                sky = np.array([settings.m_AmbientSkyColor.r, settings.m_AmbientSkyColor.g, settings.m_AmbientSkyColor.b])
                ground = np.array([settings.m_AmbientGroundColor.r, settings.m_AmbientGroundColor.g, settings.m_AmbientGroundColor.b])
                sky = np.maximum(sky, 0)**2.2
                ground = np.maximum(ground, 0)**2.2
                hemi = (n[:, 1:2] + 1) * .5
                irradiance = (ground*(1-hemi) + sky*hemi)*settings.m_AmbientIntensity
                for typ, position, forward, intensity, radius, cone in lights:
                    if typ == 1:
                        direction = np.broadcast_to(-forward, v.shape)
                        attenuation = 1.
                    else:
                        direction = position - v
                        distance = np.linalg.norm(direction, axis=1, keepdims=True)
                        direction /= np.maximum(distance, 1e-8)
                        attenuation = np.maximum(1 - distance/radius, 0)**2
                        if typ == 0:
                            spot = -direction @ forward
                            attenuation *= np.clip((spot[:, None]-cone)/max(1-cone, 1e-5), 0, 1)**2
                    irradiance += np.maximum(np.sum(n*direction, axis=1, keepdims=True), 0)*attenuation*intensity
                uv = np.array(h.m_UV0)[unique, :2] if h.m_UV0 else np.zeros((len(unique), 2))
                uv = uv*np.array(scale) + np.array(offset)
                v = (v-eye)*[1, 1, -1]  # Unity left-handed -> OpenXR right-handed.
                rows = np.column_stack((v, uv, irradiance*base[:3], np.broadcast_to(emission, v.shape), np.full(len(v), base[3])))
                uv_script = uv_nodes.get(key(t))
                scroll = tuple(uv_script[1]) if uv_script and uv_script[0] == material_index else (0., 0.)
                batches[(path, mode, cutoff, motion_chain(t), scroll)].append(rows[inverse].astype('<f4'))
                counts['triangles'] += len(ids)//3
            counts['renderers'] += 1
        draws, cursor, aliases = pack_meshes(batches, staging)
        for draw in draws + particles:
            draw['texture'] = aliases.get(draw['texture'], draw['texture'])
        compress_binaries(staging)
        manifest = {'version': 2, 'name': 'L.A.2097 – Precipice', 'author': 'DEEPFIELD / E’van Johnston',
                    'source': SOURCE, 'archive_sha256': ARCHIVE_SHA256, 'vertex_stride': 32, 'indices': 'indices.bin.zst', 'motions': motions,
                    'particles': particles, 'particle_file': 'particles.bin.zst', 'sounds': sounds,
                    'vertices': 'vertices.bin.zst', 'draws': draws, 'original_eye': eye.tolist(),
                    'fog': [settings.m_FogColor.r, settings.m_FogColor.g, settings.m_FogColor.b, settings.m_FogDensity],
                    'sky': [.0220588446, .0158026647, .013300186], 'counts': dict(counts),
                    'notes': 'Native traffic/lift/rotation/UV animation, rain/steam, compressed source audio. Indexed meshes, packed UV/lighting and losslessly optimised textures; Unity player/scripts are not executed.'}
        (staging/'scene.json').write_text(json.dumps(manifest, indent=2)+'\n')
        (staging/'SOURCE.txt').write_text(f'{manifest["name"]}\n{manifest["author"]}\n{SOURCE}\nOriginal Vive v0.9.9.4 SHA256 {ARCHIVE_SHA256}\nLocally imported assets; not licensed under DomeD MIT.\n')
        import shutil
        shutil.copytree(staging, output)
        print(json.dumps({'output': str(output), 'vertices': cursor, 'textures': len(list(staging.glob('*.png'))), 'motions': len(motions), 'particle_emitters': len(particles), 'sounds': len(sounds), **counts}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path, help='Original free LA2097 Vive ZIP from '+SOURCE)
    parser.add_argument('--output', type=Path, default=Path(os.environ.get('XDG_DATA_HOME', str(Path.home()/'.local/share')))/'domed/environments/la2097-precipice')
    args = parser.parse_args()
    convert(args.archive, args.output)


if __name__ == '__main__':
    main()
