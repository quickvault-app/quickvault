// SPDX-License-Identifier: AGPL-3.0-or-later
// Public synthetic bytes only. Native frame presentation is checked in media.js.
const {test}=require("node:test");
const assert=require("node:assert/strict");
const fs=require("node:fs");
const vm=require("node:vm");
const path=require("node:path");
const code=fs.readFileSync(path.join(__dirname,"../assets/player.js"),"utf8");
const cleanupCode=fs.readFileSync(path.join(__dirname,"../assets/app.js"),"utf8").split("function revokeHiddenPreview")[0];
const settle=()=>new Promise(resolve=>setImmediate(resolve));
function fixture(){
    const listeners=new Map(),requests=[],urls=[],revoked=[];
    class Element{
        constructor(){this.attributes=new Map();this.listeners=new Map();this.parents=new Map();this.style={setProperty(){}};}
        setAttribute(k,v){this.attributes.set(k,v);}
        getAttribute(k){return this.attributes.get(k)??null;}
        hasAttribute(k){return this.attributes.has(k);}
        removeAttribute(k){this.attributes.delete(k);}
        closest(selector){return this.parents.get(selector)??null;}
        matches(){return false;}
        addEventListener(k,fn){this.listeners.set(k,fn);}
    }
    class Media extends Element{
        constructor(){super();this.isConnected=true;this.readyState=1;this.duration=12;this.currentTime=0;this.paused=true;this.ended=false;this.played=0;this.loads=0;this.setAttribute("data-qv-src","http://127.0.0.1/synthetic-capability");this.setAttribute("data-qv-type","audio/wav");}
        set src(value){this.setAttribute("src",value);this.readyState=1;}
        get src(){return this.getAttribute("src");}
        matches(selector){return selector==="audio,video,img";}
        load(){this.loads++;this.readyState=0;this.currentTime=0;this.ended=false;}
        pause(){this.paused=true;}
        play(){this.played++;this.paused=false;this.ended=false;return Promise.resolve();}
    }
    const document={hidden:false,activeElement:null,addEventListener(k,fn){if(!listeners.has(k))listeners.set(k,[]);listeners.get(k).push(fn);}};
    const context=vm.createContext({document,window:{},Element,HTMLMediaElement:Media,HTMLAudioElement:Media,HTMLImageElement:class extends Element{},HTMLVideoElement:class extends Media{},AbortController,Uint8Array,Blob,
        matchMedia(){return {matches:false};},
        URL:{createObjectURL(blob){urls.push(blob);return `blob:synthetic-${urls.length}`;},revokeObjectURL(url){revoked.push(url);}},
        fetch(url,options){return new Promise((resolve,reject)=>requests.push({url,options,resolve,reject}));},
        requestAnimationFrame(){return 1;},setTimeout,clearTimeout});
    const api=vm.runInContext(code+"\n"+cleanupCode+"\n({loadBlob,dropBlob,wirePlayer,stopMedia});",context);
    function answer(index,bytes){requests[index].resolve({ok:true,arrayBuffer:()=>Promise.resolve(bytes.buffer)});}
    function event(name,target,detail=1){
        const value={target,detail,prevented:false,stopped:false,preventDefault(){this.prevented=true;},stopImmediatePropagation(){this.stopped=true;}};
        for(const listener of listeners.get(name)??[]){listener(value);if(value.stopped)break;}
        return value;
    }
    return{...api,Media,Element,requests,urls,revoked,document,answer,event};
}
test("explicit replay starts only after a fresh memory source is attached",async()=>{
    const f=fixture(),media=new f.Media(),bytes=new Uint8Array([1,2,3]);
    f.loadBlob(media,true);
    assert.equal(media.played,0);
    assert.equal(f.requests[0].options.cache,"no-store");
    f.answer(0,bytes);await settle();
    assert.equal(media.src,"blob:synthetic-1");assert.equal(media.played,1);
    assert.deepEqual([...bytes],[0,0,0]);
    f.dropBlob(media);
    assert.deepEqual(f.revoked,["blob:synthetic-1"]);
});
test("close or lock during replay aborts the request and wipes a late response",async()=>{
    for(const connected of [false,true]){
        const f=fixture(),media=new f.Media(),bytes=new Uint8Array([7,8]);
        f.loadBlob(media,true);f.dropBlob(media);media.isConnected=connected;
        assert.equal(f.requests[0].options.signal.aborted,true);
        f.answer(0,bytes);await settle();
        assert.deepEqual([...bytes],[0,0]);assert.equal(f.urls.length,0);assert.equal(media.played,0);
    }
});
test("a replaced playback load cannot attach or seek over a newer load",async()=>{
    const f=fixture(),media=new f.Media(),oldBytes=new Uint8Array([3]),newBytes=new Uint8Array([4]);
    f.loadBlob(media,true,1);f.loadBlob(media,false,2);
    f.answer(1,newBytes);await settle();
    f.answer(0,oldBytes);await settle();
    assert.equal(f.requests[0].options.signal.aborted,true);
    assert.equal(f.urls.length,1);assert.equal(media.played,0);
    assert.equal(media.currentTime,2);
    assert.deepEqual([...oldBytes],[0]);assert.deepEqual([...newBytes],[0]);
});
test("a refused replay cannot start media or leave an active load",async()=>{
    const f=fixture(),media=new f.Media();f.loadBlob(media,true);
    f.requests[0].reject(new Error("synthetic refusal"));await settle();
    assert.equal(f.requests[0].options.signal.aborted,true);
    assert.equal(media.played,0);assert.equal(f.urls.length,0);
});
test("cleanup clears a retained default playback position after detaching the source",async()=>{
    const f=fixture(),media=new f.Media();f.loadBlob(media);f.answer(0,new Uint8Array([1]));await settle();
    media.currentTime=12;media.paused=false;
    media.load=function(){this.readyState=0;}; // Engine retains its old position.
    f.stopMedia(media);
    assert.equal(media.paused,true);assert.equal(media.hasAttribute("src"),false);
    assert.equal(media.currentTime,0);assert.equal(f.requests[0].options.signal.aborted,true);
    assert.deepEqual(f.revoked,["blob:synthetic-1"]);
});
test("the play control rebuilds only an ended memory source",async()=>{
    const f=fixture(),media=new f.Media(),player=new f.Element();
    const play=new f.Element(),seek=new f.Element(),time=new f.Element();
    player.querySelector=s=>({"audio,video":media,".v-play":play,".v-seek":seek,".v-time":time}[s]??null);
    f.wirePlayer(player);
    const click=()=>play.listeners.get("click")();
    click();assert.equal(media.played,1);assert.equal(f.requests.length,0);
    click();assert.equal(media.paused,true);assert.equal(f.requests.length,0);
    media.ended=true;click();assert.equal(f.requests.length,1);assert.equal(media.played,1);
    f.answer(0,new Uint8Array([1]));await settle();assert.equal(media.played,2);
    media.ended=true;media.removeAttribute("data-qv-src");click();
    assert.equal(media.played,3);assert.equal(f.requests.length,1);
});
function seekControl(f,media){
    const player=new f.Element(),play=new f.Element(),seek=new f.Element(),time=new f.Element();
    player.querySelector=s=>({"audio,video":media,".v-play":play,".v-seek":seek,".v-time":time}[s]??null);
    f.wirePlayer(player);
    return position=>{seek.value=String(position);seek.listeners.get("input")();seek.listeners.get("change")();};
}
test("an ended memory source restores the slider position while staying paused",async()=>{
    for(const target of [1,0]){
        const f=fixture(),media=new f.Media();media.src="blob:exhausted";media.currentTime=12;media.ended=true;
        seekControl(f,media)(target);
        assert.equal(f.requests.length,1);assert.equal(media.played,0);
        f.answer(0,new Uint8Array([1]));await settle();
        assert.equal(media.currentTime,target);assert.equal(media.paused,true);assert.equal(media.played,0);
    }
});
test("ordinary seeks preserve the source and playback state",()=>{
    for(const paused of [false,true]){
        const f=fixture(),media=new f.Media();media.paused=paused;media.src="blob:current";
        seekControl(f,media)(2);
        assert.equal(media.currentTime,2);assert.equal(media.paused,paused);
        assert.equal(media.src,"blob:current");assert.equal(f.requests.length,0);
    }
    const f=fixture(),media=new f.Media();media.removeAttribute("data-qv-src");media.ended=true;
    seekControl(f,media)(2);assert.equal(media.currentTime,2);assert.equal(f.requests.length,0);
});
test("slider targets are bounded and an end-position selection does not reload",()=>{
    const f=fixture(),media=new f.Media(),seek=seekControl(f,media);media.currentTime=5;
    seek(NaN);assert.equal(media.currentTime,5);assert.equal(f.requests.length,0);
    seek(-2);assert.equal(media.currentTime,0);
    media.currentTime=12;media.ended=true;
    let writes=0;Object.defineProperty(media,"currentTime",{get(){return 12;},set(){writes++;this.ended=false;}});
    seek(100);assert.equal(media.currentTime,12);assert.equal(f.requests.length,0);
    assert.equal(writes,0);assert.equal(media.ended,true);
});
test("close or lock cancels a pending seek position as well as playback",async()=>{
    for(const connected of [false,true]){
        const f=fixture(),media=new f.Media();media.src="blob:exhausted";media.currentTime=12;media.ended=true;
        seekControl(f,media)(1);assert.equal(f.requests.length,1);
        f.dropBlob(media);media.isConnected=connected;
        const bytes=new Uint8Array([5]);f.answer(0,bytes);await settle();
        assert.equal(media.currentTime,0);assert.equal(media.played,0);assert.equal(f.urls.length,0);
        assert.deepEqual([...bytes],[0]);assert.equal(f.requests[0].options.signal.aborted,true);
    }
});
test("a player drag cannot click the backdrop, while ordinary and keyboard clicks work",()=>{
    const f=fixture(),control=new f.Element(),backdrop=new f.Element();
    control.parents.set(".v-player",new f.Element());backdrop.parents.set(".viewer",new f.Element());
    f.event("pointerdown",control);
    const released=f.event("click",backdrop);assert.equal(released.stopped,true);assert.equal(released.prevented,true);
    f.event("pointerdown",backdrop);assert.equal(f.event("click",backdrop).stopped,false);
    f.event("pointerdown",control);f.event("pointercancel",control);assert.equal(f.event("click",backdrop).stopped,false);
    f.event("pointerdown",control);assert.equal(f.event("click",backdrop,0).stopped,false);
    f.event("pointerdown",control);assert.equal(f.event("click",control).stopped,false);
});
