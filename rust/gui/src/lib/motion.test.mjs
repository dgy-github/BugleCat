import {test} from 'node:test';
import assert from 'node:assert/strict';
import {pose} from './motion.js';
test('neutral pose',()=>assert.deepEqual(pose(0,0),{rx:-0,ry:0,x:0,y:0}));
test('bounded tilt and movement',()=>assert.deepEqual(pose(20,-9),{rx:6,ry:6,x:12,y:-12}));
test('invalid values stay neutral',()=>assert.equal(pose(NaN,Infinity).x,0));
test('hero never tilts',()=>assert.equal(pose(1,1,{tilt:0,shift:14}).ry,0));
test('opposite directions are symmetric',()=>assert.equal(pose(-.7,0).ry,-pose(.7,0).ry));
