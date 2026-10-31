#!/usr/bin/env python3
"""Decode every hardware fixture frame and verify eye, orientation and colour."""
import json,subprocess,sys
from pathlib import Path
directory=Path(sys.argv[1]);checks=[]
codec=sys.argv[2]if len(sys.argv)>2 else'h264'
expected_codec='hevc'if codec=='h265'else codec
for shape in ('flat','curved'):
    for eye in ('left','right'):
        path=directory/f'{shape}-{eye}.mkv'
        info=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_streams','-of','json',path]))['streams'][0]
        assert info['codec_name']==expected_codec and info['color_space']=='bt709'
        w,h=info['width'],info['height'];size=w*h*3
        raw=subprocess.check_output(['ffmpeg','-v','error','-i',path,'-f','rawvideo','-pix_fmt','rgb24','-'])
        assert len(raw)==40*size,(path,len(raw),size)
        expected=(240,32,16)if eye=='left'else(16,32,240)
        for offset in range(0,len(raw),size):
            center=tuple(raw[offset+(h//2*w+w//2)*3:offset+(h//2*w+w//2)*3+3])
            top=tuple(raw[offset+(h//12*w+w//2)*3:offset+(h//12*w+w//2)*3+3])
            assert all(abs(a-b)<12 for a,b in zip(center,expected)),(path,center,expected)
            assert top[1]>200 and top[0]<40 and top[2]<40,(path,top)
        checks.append({'file':str(path),'decoded_frames':40,'eye':eye,'shape':shape,'passed':True})
        print('PASS',path.name,'40 decoded frames, selected eye, correct orientation and RGB colours')
(directory/'decoded-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
