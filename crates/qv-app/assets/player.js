// SPDX-License-Identifier: AGPL-3.0-or-later
// The viewer's media player (0003 §6.4, 0009 §1.8, plan 0057): the app's own
// controls on every platform and, on Linux, playback of verified media from
// memory. No dynamic code, no logging, no media bytes kept after playback.

// WebKitGTK buffers playing HTTP media to an unlinked file in /var/tmp, but
// not media appended through Media Source Extensions or played from a blob.
// On Linux the viewer gives media a data-qv-src capability instead of src:
// fetch the verified bytes into memory once, convert an MP4 into fragments for
// a MediaSource (WebKit's blob playback cannot play an MP4 whose index is at
// the end, and its replay after the end sticks), and fall back to a blob URL
// for other files. Everything is dropped with the preview.
const blobs=new WeakMap();
function dropBlob(media){
    const held=blobs.get(media);
    if(!held)return;
    blobs.delete(media);
    held.abort.abort();
    if(held.url)URL.revokeObjectURL(held.url);
    held.wipe();
}
function loadBlob(media){
    dropBlob(media);
    // A reused element must not keep playing the previous file.
    if(media.hasAttribute("src")){media.pause();media.removeAttribute("src");media.load();}
    const source=media.getAttribute("data-qv-src");
    if(!source||document.hidden)return;
    const type=media.getAttribute("data-qv-type")||"";
    const held={abort:new AbortController(),url:null,wipe(){}};
    blobs.set(media,held);
    fetch(source,{cache:"no-store",credentials:"omit",referrerPolicy:"no-referrer",signal:held.abort.signal})
        .then(response=>{if(!response.ok)throw new Error("preview unavailable");return response.arrayBuffer();})
        .then(buffer=>{
            const bytes=new Uint8Array(buffer);
            if(blobs.get(media)!==held||!media.isConnected){bytes.fill(0);return;}
            playFromMemory(media,held,bytes,type);
        })
        .catch(()=>{if(blobs.get(media)===held)dropBlob(media);});
}
function loadBlobs(records){
    for(const record of records){
        if(record.type==="attributes"&&record.attributeName==="data-qv-src"){loadBlob(record.target);continue;}
        for(const node of record.addedNodes){
            if(!(node instanceof Element))continue;
            if(node.matches("audio[data-qv-src],video[data-qv-src]"))loadBlob(node);
            for(const media of node.querySelectorAll("audio[data-qv-src],video[data-qv-src]"))loadBlob(media);
        }
    }
}
function playBlob(media,held,bytes,type){
    media.setAttribute("data-qv-playback","file");
    held.url=URL.createObjectURL(new Blob([bytes],{type}));
    bytes.fill(0);
    media.src=held.url;
}
function playFromMemory(media,held,bytes,type){
    let plan=null;
    if(window.MediaSource&&(type==="video/mp4"||type==="audio/mp4")){
        try{plan=fragmentMp4(bytes,type);}catch{plan=null;}
        if(plan&&!MediaSource.isTypeSupported(plan.type))plan=null;
    }
    if(!plan){playBlob(media,held,bytes,type);return;}
    // Wiped once appended, or when the preview closes first (best effort).
    held.wipe=()=>{bytes.fill(0);for(const part of plan.parts)part.fill(0);};
    const source=new MediaSource();
    media.setAttribute("data-qv-playback","stream");
    held.url=URL.createObjectURL(source);
    source.addEventListener("sourceopen",async()=>{
        URL.revokeObjectURL(held.url);held.url=null;
        try{
            const buffer=source.addSourceBuffer(plan.type);
            for(const part of plan.parts){
                if(blobs.get(media)!==held)return;
                await new Promise((done,fail)=>{
                    buffer.addEventListener("updateend",done,{once:true});
                    buffer.addEventListener("error",fail,{once:true});
                    buffer.appendBuffer(part);
                });
                // appendBuffer copied it; wipe ours (best effort).
                part.fill(0);
            }
            if(blobs.get(media)===held&&source.readyState==="open")source.endOfStream();
        }catch{
            // The engine refused the fragments: play the file itself instead.
            if(blobs.get(media)===held){media.removeAttribute("src");media.load();playBlob(media,held,bytes,type);}
        }finally{held.wipe();}
    },{once:true});
    media.src=held.url;
}

// Lossless MP4 → fragmented MP4 conversion for a MediaSource. It reads the
// sample tables of the audio and video tracks and rewrites the same coded
// samples as an init segment plus ~1 s fragments; nothing is re-encoded.
// The input is bounded (a preview is at most 10 MiB) and every offset is
// checked; anything unexpected throws and the caller plays the blob instead.
const MAX_SAMPLES=1<<21;
function fragmentMp4(bytes,type){
    const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
    const top=boxes(view,0,bytes.byteLength);
    const moov=only(top,"moov");
    const inMoov=boxes(view,moov.body,moov.end);
    const mvhd=only(inMoov,"mvhd");
    const tracks=[];
    for(const trak of inMoov.filter(b=>b.type==="trak")){
        const track=readTrack(view,bytes,trak);
        if(track)tracks.push(track);
    }
    if(!tracks.length)throw new Error("mp4");
    const video=tracks.some(t=>t.handler==="vide");
    if(type==="audio/mp4"&&video)throw new Error("mp4");
    const init=concat(
        box("ftyp",ascii("iso5"),u32(512),ascii("iso5iso6mp41")),
        box("moov",raw(bytes,mvhd),...tracks.map(t=>initTrack(bytes,t)),
            box("mvex",...tracks.map(t=>full("trex",0,0,u32(t.id),u32(1),u32(0),u32(0),u32(0))))));
    const fragments=[];
    for(const track of tracks)for(const run of runs(track))fragments.push(run);
    fragments.sort((a,b)=>a.start-b.start);
    const parts=[init];
    fragments.forEach((run,i)=>parts.push(fragment(bytes,run,i+1)));
    const codecs=tracks.map(t=>t.codec).join(",");
    return{type:`${video?"video":"audio"}/mp4; codecs="${codecs}"`,parts};
}
function boxes(view,start,end){
    const out=[];
    let at=start;
    while(at+8<=end){
        let size=view.getUint32(at),head=8;
        const type=String.fromCharCode(view.getUint8(at+4),view.getUint8(at+5),view.getUint8(at+6),view.getUint8(at+7));
        if(size===1){
            if(at+16>end||view.getUint32(at+8)!==0)throw new Error("mp4");
            size=view.getUint32(at+12);head=16;
        }else if(size===0)size=end-at;
        if(size<head||at+size>end||out.length>=4096)throw new Error("mp4");
        out.push({type,start:at,body:at+head,end:at+size});
        at+=size;
    }
    return out;
}
function only(list,type){
    const found=list.filter(b=>b.type===type);
    if(found.length!==1)throw new Error("mp4");
    return found[0];
}
function find(list,type){return list.find(b=>b.type===type)??null;}
function readTrack(view,bytes,trak){
    const inTrak=boxes(view,trak.body,trak.end);
    const tkhd=only(inTrak,"tkhd");
    const mdia=only(inTrak,"mdia");
    const inMdia=boxes(view,mdia.body,mdia.end);
    const hdlr=only(inMdia,"hdlr");
    const handler=fourcc(view,hdlr.body+8);
    if(handler!=="vide"&&handler!=="soun")return null;
    const mdhd=only(inMdia,"mdhd");
    const scale=view.getUint32(mdhd.body+(view.getUint8(mdhd.body)===1?20:12));
    const minf=only(inMdia,"minf");
    const inMinf=boxes(view,minf.body,minf.end);
    const stbl=only(inMinf,"stbl");
    const inStbl=boxes(view,stbl.body,stbl.end);
    const stsd=only(inStbl,"stsd");
    // A track without samples (such as an empty audio track) is left out.
    const stsz=only(inStbl,"stsz");
    if(view.getUint32(stsz.body+8)===0)return null;
    if(!scale||view.getUint32(stsd.body+4)!==1)throw new Error("mp4");
    const entry=boxes(view,stsd.body+8,stsd.end)[0];
    if(!entry)throw new Error("mp4");
    const codec=codecOf(view,entry,handler);
    const id=view.getUint32(tkhd.body+(view.getUint8(tkhd.body)===1?20:12));
    const samples=sampleTable(view,bytes.byteLength,inStbl);
    // Each track starts at 0 (the first presented sample), as FFmpeg's
    // fragmenting does: WebKit stalls when it seeks into a gap at the start,
    // so an edit list's few milliseconds of encoder delay are not applied.
    let decode=0,earliest=Infinity;
    for(let i=0;i<samples.count;i++){earliest=Math.min(earliest,decode+samples.composition[i]);decode+=samples.duration[i];}
    const shift=-earliest;
    if(!Number.isSafeInteger(shift)||Math.abs(shift)>0x3fffffff)throw new Error("mp4");
    return{id,handler,scale,codec,samples,shift,trak,inTrak,mdia,inMdia,minf,inMinf,stsd};
}
function sampleTable(view,length,inStbl){
    const stsz=only(inStbl,"stsz");
    const fixed=view.getUint32(stsz.body+4),count=view.getUint32(stsz.body+8);
    if(count>MAX_SAMPLES||(!fixed&&stsz.body+12+count*4>stsz.end))throw new Error("mp4");
    const size=new Uint32Array(count);
    for(let i=0;i<count;i++)size[i]=fixed||view.getUint32(stsz.body+12+i*4);
    const offset=new Float64Array(count);
    const stco=find(inStbl,"stco"),co64=find(inStbl,"co64");
    const chunks=stco??co64;
    if(!chunks||(stco&&co64))throw new Error("mp4");
    const chunkCount=view.getUint32(chunks.body+4),wide=!!co64;
    if(chunks.body+8+chunkCount*(wide?8:4)>chunks.end)throw new Error("mp4");
    const stsc=only(inStbl,"stsc");
    const runs=view.getUint32(stsc.body+4);
    if(runs===0||stsc.body+8+runs*12>stsc.end)throw new Error("mp4");
    let sample=0;
    for(let r=0;r<runs&&sample<count;r++){
        const at=stsc.body+8+r*12;
        const first=view.getUint32(at),per=view.getUint32(at+4);
        const last=r+1<runs?view.getUint32(at+12)-1:chunkCount;
        if(first<1||last>chunkCount||(last<first-1))throw new Error("mp4");
        for(let chunk=first;chunk<=last&&sample<count;chunk++){
            let at2=wide?u64(view,chunks.body+8+(chunk-1)*8):view.getUint32(chunks.body+8+(chunk-1)*4);
            for(let k=0;k<per&&sample<count;k++){
                if(at2+size[sample]>length)throw new Error("mp4");
                offset[sample]=at2;at2+=size[sample];sample++;
            }
        }
    }
    if(sample!==count)throw new Error("mp4");
    const duration=new Uint32Array(count);
    const stts=only(inStbl,"stts");
    const timed=view.getUint32(stts.body+4);
    if(stts.body+8+timed*8>stts.end)throw new Error("mp4");
    sample=0;
    for(let e=0;e<timed&&sample<count;e++){
        const n=view.getUint32(stts.body+8+e*8),delta=view.getUint32(stts.body+12+e*8);
        for(let k=0;k<n&&sample<count;k++)duration[sample++]=delta;
    }
    // A missing last delta repeats the previous one.
    for(;sample<count;sample++)duration[sample]=sample?duration[sample-1]:0;
    const composition=new Int32Array(count);
    const ctts=find(inStbl,"ctts");
    if(ctts){
        const n=view.getUint32(ctts.body+4);
        if(ctts.body+8+n*8>ctts.end)throw new Error("mp4");
        sample=0;
        for(let e=0;e<n&&sample<count;e++){
            const k=view.getUint32(ctts.body+8+e*8),value=view.getInt32(ctts.body+12+e*8);
            for(let j=0;j<k&&sample<count;j++)composition[sample++]=value;
        }
    }
    let sync=null;
    const stss=find(inStbl,"stss");
    if(stss){
        const n=view.getUint32(stss.body+4);
        if(stss.body+8+n*4>stss.end)throw new Error("mp4");
        sync=new Uint8Array(count);
        for(let e=0;e<n;e++){const s=view.getUint32(stss.body+8+e*4);if(s>=1&&s<=count)sync[s-1]=1;}
    }
    return{count,size,offset,duration,composition,sync};
}
// Fragments of about one second; a video fragment starts at a sync sample.
function runs(track){
    const{count,duration,sync}=track.samples;
    const out=[];
    let first=0,time=0,start=0;
    for(let i=0;i<=count;i++){
        const cut=i===count||(i>first&&time-start>=track.scale&&(!sync||sync[i]));
        if(cut){out.push({track,first,end:i,decode:start,start:start/track.scale});first=i;start=time;}
        if(i<count)time+=duration[i];
    }
    return out;
}
function fragment(bytes,run,sequence){
    const{track,first,end}=run,s=track.samples,n=end-first;
    let length=0;
    for(let i=first;i<end;i++)length+=s.size[i];
    const trunSize=20+n*16;
    const moofSize=8+16+8+16+20+trunSize;
    const out=new Uint8Array(moofSize+8+length);
    const view=new DataView(out.buffer);
    let at=0;
    const put32=v=>{view.setUint32(at,v>>>0);at+=4;};
    const tag=t=>{for(let i=0;i<4;i++)out[at++]=t.charCodeAt(i);};
    put32(moofSize);tag("moof");
    put32(16);tag("mfhd");put32(0);put32(sequence);
    put32(8+16+20+trunSize);tag("traf");
    // default-base-is-moof
    put32(16);tag("tfhd");put32(0x020000);put32(track.id);
    put32(20);tag("tfdt");put32(0x01000000);put32(Math.floor(run.decode/2**32));put32(run.decode%2**32);
    // version 1: signed composition offsets; per-sample duration, size, flags, offset
    put32(trunSize);tag("trun");put32(0x01000f01);put32(n);put32(moofSize+8);
    for(let i=first;i<end;i++){
        put32(s.duration[i]);put32(s.size[i]);
        put32(!s.sync||s.sync[i]?0x02000000:0x01010000);
        view.setInt32(at,s.composition[i]+track.shift);at+=4;
    }
    put32(8+length);tag("mdat");
    for(let i=first;i<end;i++){out.set(bytes.subarray(s.offset[i],s.offset[i]+s.size[i]),at);at+=s.size[i];}
    return out;
}
function initTrack(bytes,t){
    // The original track, header and sample description, with empty sample
    // tables; the edit list and other boxes are dropped.
    const empty=[full("stts",0,0,u32(0)),full("stsc",0,0,u32(0)),full("stsz",0,0,u32(0),u32(0)),full("stco",0,0,u32(0))];
    const minf=t.inMinf.filter(b=>b.type!=="stbl").map(b=>raw(bytes,b));
    const mdia=t.inMdia.filter(b=>b.type==="mdhd"||b.type==="hdlr").map(b=>raw(bytes,b));
    return box("trak",raw(bytes,only(t.inTrak,"tkhd")),
        box("mdia",...mdia,box("minf",...minf,box("stbl",raw(bytes,t.stsd),...empty))));
}
// RFC 6381 codec names for the sample entry, as MediaSource requires.
function codecOf(view,entry,handler){
    const kind=fourcc(view,entry.start+4);
    const hex=v=>v.toString(16).padStart(2,"0");
    if(handler==="vide"){
        const config=boxes(view,entry.body+78,entry.end);
        if(kind==="avc1"||kind==="avc3"){
            const c=only(config,"avcC").body;
            return `${kind}.${hex(view.getUint8(c+1))}${hex(view.getUint8(c+2))}${hex(view.getUint8(c+3))}`;
        }
        if(kind==="hvc1"||kind==="hev1"){
            const c=only(config,"hvcC").body;
            const b=view.getUint8(c+1),flags=view.getUint32(c+2);
            let reversed=0;
            for(let i=0;i<32;i++)if(flags&(1<<i))reversed|=1<<(31-i);
            const constraints=[];
            for(let i=0;i<6;i++)constraints.push(view.getUint8(c+6+i));
            while(constraints.length&&!constraints[constraints.length-1])constraints.pop();
            return [`${kind}.${["","A","B","C"][b>>6]}${b&31}`,(reversed>>>0).toString(16),`${b&32?"H":"L"}${view.getUint8(c+12)}`,...constraints.map(v=>v.toString(16).toUpperCase())].join(".");
        }
        if(kind==="vp09"){
            const c=only(config,"vpcC").body;
            const two=v=>String(v).padStart(2,"0");
            return `vp09.${two(view.getUint8(c+4))}.${two(view.getUint8(c+5))}.${two(view.getUint8(c+6)>>4)}`;
        }
        if(kind==="av01"){
            const c=only(config,"av1C").body;
            const a=view.getUint8(c+1),b=view.getUint8(c+2);
            const depth=b&64?(b&32?12:10):8;
            return `av01.${a>>5}.${String(a&31).padStart(2,"0")}${b&128?"H":"M"}.${String(depth).padStart(2,"0")}`;
        }
        throw new Error("mp4");
    }
    if(kind==="Opus")return "opus";
    if(kind==="fLaC")return "flac";
    if(kind!=="mp4a")throw new Error("mp4");
    // QuickTime sound descriptions v1/v2 have 16/36 more bytes before the boxes.
    const version=view.getUint16(entry.body+8);
    const extra=version===1?16:version===2?36:0;
    const esds=only(boxes(view,entry.body+28+extra,entry.end),"esds");
    return esdsCodec(view,esds.body+4,esds.end);
}
function esdsCodec(view,at,end){
    const tag=want=>{
        if(at>=end||view.getUint8(at)!==want)throw new Error("mp4");
        at++;let size=0;
        for(let i=0;i<4;i++){const b=view.getUint8(at++);size=size*128+(b&127);if(!(b&128))break;}
        if(at+size>end)throw new Error("mp4");
        return size;
    };
    tag(3);
    const flags=view.getUint8(at+2);at+=3;
    if(flags&128)at+=2;
    if(flags&64)at+=1+view.getUint8(at);
    if(flags&32)at+=2;
    tag(4);
    const object=view.getUint8(at);at+=13;
    if(object!==0x40)return `mp4a.${object.toString(16).toUpperCase()}`;
    tag(5);
    let profile=view.getUint8(at)>>3;
    if(profile===31)profile=32+((view.getUint16(at)>>5)&63);
    return `mp4a.40.${profile}`;
}
function fourcc(view,at){return String.fromCharCode(view.getUint8(at),view.getUint8(at+1),view.getUint8(at+2),view.getUint8(at+3));}
function u64(view,at){const v=view.getUint32(at)*2**32+view.getUint32(at+4);if(!Number.isSafeInteger(v))throw new Error("mp4");return v;}
function u32(v){const out=new Uint8Array(4);new DataView(out.buffer).setUint32(0,v>>>0);return out;}
function ascii(text){return Uint8Array.from(text,c=>c.charCodeAt(0));}
function raw(bytes,b){return bytes.subarray(b.start,b.end);}
function concat(...parts){
    const out=new Uint8Array(parts.reduce((n,p)=>n+p.length,0));
    let at=0;
    for(const p of parts){out.set(p,at);at+=p.length;}
    return out;
}
function box(type,...parts){
    const body=concat(...parts);
    const out=new Uint8Array(8+body.length);
    new DataView(out.buffer).setUint32(0,out.length);
    out.set(ascii(type),4);out.set(body,8);
    return out;
}
function full(type,version,flags,...parts){return box(type,u32(version<<24|flags),...parts);}

// The player's controls are rendered by the app (Rust); this wires them to
// the media element: play/pause, seek, time, mute and volume. Only playback
// state is shown; nothing is sent anywhere.
const wired=new WeakSet();
function clock(seconds){
    if(!Number.isFinite(seconds))return "0:00";
    const s=Math.floor(seconds),h=Math.floor(s/3600),m=Math.floor(s/60)%60,r=String(s%60).padStart(2,"0");
    return h?`${h}:${String(m).padStart(2,"0")}:${r}`:`${m}:${r}`;
}
function wirePlayer(player){
    if(wired.has(player))return;
    const media=player.querySelector("audio,video");
    const play=player.querySelector(".v-play"),seek=player.querySelector(".v-seek");
    const time=player.querySelector(".v-time"),mute=player.querySelector(".v-mute"),volume=player.querySelector(".v-volume");
    if(!media||!play||!seek||!time)return;
    wired.add(player);
    let dragging=false,frame=0,idle=0,click=0;
    const ready=()=>media.readyState>=1&&Number.isFinite(media.duration);
    const draw=()=>{
        const duration=ready()?media.duration:0;
        if(!dragging){seek.max=String(duration||1);seek.value=String(Math.min(media.currentTime,duration||0));}
        seek.style.setProperty("--p",`${duration?100*Number(seek.value)/duration:0}%`);
        time.textContent=`${clock(dragging?Number(seek.value):media.currentTime)} / ${clock(duration)}`;
    };
    const tick=()=>{draw();frame=media.paused?0:requestAnimationFrame(tick);};
    const state=()=>{
        if(ready())player.setAttribute("data-qv-ready","");else player.removeAttribute("data-qv-ready");
        const playing=!media.paused&&!media.ended;
        player.setAttribute("data-state",playing?"playing":"paused");
        play.setAttribute("aria-label",playing?"Pause":"Play");
        play.title=playing?"Pause (Space)":"Play (Space)";
        if(mute){const quiet=media.muted||media.volume===0;player.toggleAttribute("data-muted",quiet);mute.setAttribute("aria-label",quiet?"Unmute":"Mute");mute.title=quiet?"Unmute (M)":"Mute (M)";}
        if(volume)volume.style.setProperty("--p",`${media.muted?0:100*media.volume}%`);
        if(playing&&!frame)frame=requestAnimationFrame(tick);
        draw();
    };
    // While a video plays, its controls hide after a still pointer.
    const wake=()=>{
        clearTimeout(idle);player.removeAttribute("data-idle");
        if(!(media instanceof HTMLVideoElement)||media.paused)return;
        idle=setTimeout(()=>{
            const focused=document.activeElement?.closest(".v-ctl");
            if(!media.paused&&!(focused&&player.contains(focused)))player.setAttribute("data-idle","");
        },2500);
    };
    const toggle=()=>{
        if(!ready())return;
        if(media.paused||media.ended)media.play().catch(()=>{});else media.pause();
    };
    for(const name of ["play","pause","ended","emptied","loadedmetadata","durationchange","volumechange","seeked","timeupdate"])media.addEventListener(name,state);
    for(const name of ["play","pause","ended"])media.addEventListener(name,wake);
    play.addEventListener("click",toggle);
    if(media instanceof HTMLVideoElement){
        // A double click goes full screen (Rust), so a single click waits.
        media.addEventListener("click",()=>{clearTimeout(click);click=setTimeout(toggle,220);});
        media.addEventListener("dblclick",()=>{clearTimeout(click);player.querySelector(".v-fs")?.click();});
    }
    seek.addEventListener("input",()=>{dragging=true;draw();});
    seek.addEventListener("change",()=>{dragging=false;if(ready())media.currentTime=Number(seek.value);draw();});
    mute?.addEventListener("click",()=>{if(media.muted||media.volume===0){media.muted=false;if(media.volume===0)media.volume=1;}else media.muted=true;});
    volume?.addEventListener("input",()=>{media.volume=Number(volume.value);media.muted=media.volume===0;});
    for(const name of ["pointermove","pointerdown","keydown","focusin"])player.addEventListener(name,wake,{passive:true});
    state();
}
function wirePlayers(){for(const player of document.querySelectorAll(".v-player"))wirePlayer(player);}
// Space or K plays and pauses, M mutes and F toggles full screen while a
// player is open, unless a control or a field has the key.
document.addEventListener("keydown",event=>{
    if(event.ctrlKey||event.metaKey||event.altKey||!(event.target instanceof Element))return;
    const player=document.querySelector(".viewer .v-player");
    if(!player||event.target.closest("button,input,textarea,select"))return;
    const key=event.key.toLowerCase();
    const press=selector=>{const el=player.querySelector(selector)??document.querySelector(selector);if(el){event.preventDefault();el.click();}};
    if(key===" "||key==="k")press(".v-play");
    else if(key==="m")press(".v-mute");
    else if(key==="f")press(".v-fs");
});
