// SPDX-License-Identifier: AGPL-3.0-or-later
// Native-engine test driver. All media and vault data are synthetic; never log URLs.
(async()=>{
const wait=async test=>{const end=performance.now()+60000;while(performance.now()<end){if(test())return;await new Promise(r=>setTimeout(r,100));}throw new Error("synthetic media deadline")};
const tap=async id=>{await wait(()=>!!document.getElementById(id)&&!document.getElementById(id).disabled);document.getElementById(id).click()};
const open=async name=>{await wait(()=>!!document.querySelector(`[aria-label="Preview ${name}"]`));document.querySelector(`[aria-label="Preview ${name}"]`).click()};
// currentSrc can retain a stale URL after load() without a source (WHATWG
// html#10410). Assert the decoder/network reset, not that reporting quirk.
const reset=media=>media.paused&&!media.hasAttribute("src")&&media.readyState===0&&
 (media.networkState===0||media.networkState===3)&&media.currentTime===0&&!media.isConnected;
const refused=async url=>{const late=new Audio();late.muted=true;late.preload="auto";late.src=url;late.load();try{await wait(()=>!!late.error);}finally{late.removeAttribute("src");late.load();}};
// The player's own controls are ready once the media has its metadata (player.js).
const ready=media=>wait(()=>media.closest(".v-player")?.hasAttribute("data-qv-ready"));
const press=(media,selector)=>media.closest(".v-player").querySelector(selector).click();
const playing=media=>media.closest(".v-player").getAttribute("data-state")==="playing";
let stage=1,passed=false,lastMedia=null;
const mark=n=>{stage=n;const result=document.getElementById("validation-result");if(result){result.value=`MEDIA-STAGE:${n}`;result.dispatchEvent(new Event("input",{bubbles:true}));}};
try{
 await wait(()=>!!document.querySelector(".shelf"));
 mark(2);await open("a-audio.wav");await wait(()=>!!document.querySelector(".viewer audio"));
 const audio=document.querySelector(".viewer audio");lastMedia=audio;
 // Linux plays verified media from memory (WebKitGTK buffers HTTP media to disk; plan 0057).
 const blobbed=audio.hasAttribute("data-qv-src");const audioURL=blobbed?audio.getAttribute("data-qv-src"):audio.src;
 await ready(audio);const audioSrc=audio.src;
 if(blobbed!==audioSrc.startsWith("blob:"))throw new Error("media source");
 if(audio.autoplay||!audio.paused||audio.controls)throw new Error("eager media");
 audio.muted=true;press(audio,".v-play");await wait(()=>audio.currentTime>0.3&&playing(audio));
 mark(3);audio.currentTime=31;await wait(()=>!audio.seeking&&audio.currentTime>31.2);
 mark(4);await tap("viewer-close");await wait(()=>!document.querySelector(".viewer"));
 await wait(()=>reset(audio));mark(40);await refused(audioURL);if(blobbed)await refused(audioSrc);mark(41);
 mark(5);await open("b-video.mp4");await wait(()=>!!document.querySelector(".viewer video"));
 const video=document.querySelector(".viewer video");lastMedia=video;
 if(video.preload!=="auto"||video.autoplay)throw new Error("video preload");
 await ready(video);if(blobbed!==video.src.startsWith("blob:"))throw new Error("media source");
 // An MP4 plays through Media Source Extensions, not WebKit's blob playback.
 if(blobbed&&video.getAttribute("data-qv-playback")!=="stream")throw new Error("media stream");
 // The first frame is ready before play, and the player fills the stage (plan 0057).
 await wait(()=>video.readyState>=2&&video.videoWidth===128);
 if(!video.paused||video.currentTime!==0||video.clientWidth<=300)throw new Error("video idle");
 video.muted=true;press(video,".v-play");await wait(()=>video.currentTime>0.3&&playing(video));
 mark(6);video.currentTime=2;await wait(()=>!video.seeking&&video.currentTime>2.2);
 // Play again after the end starts over and advances (plan 0057).
 await wait(()=>video.ended&&!playing(video));press(video,".v-play");
 await wait(()=>!video.paused&&video.currentTime>0.3&&video.currentTime<3);
 // Full screen is the player's own control (desktop).
 if(document.getElementById("viewer-full")){
  await tap("viewer-full");await wait(()=>!!document.querySelector(".viewer.full"));
  await tap("viewer-full");await wait(()=>!document.querySelector(".viewer.full"));
 }
 mark(7);await tap("viewer-close");await wait(()=>reset(video));
 mark(8);await open("c-image.png");await wait(()=>!!document.querySelector(".viewer img"));
 await wait(()=>document.querySelector(".viewer img").naturalWidth===64);
 await tap("viewer-close");await wait(()=>!document.querySelector(".viewer"));
 mark(9);await open("d-large.png");await wait(()=>!!document.getElementById("viewer-load-image"));
 if(document.querySelector(".viewer img")||!document.querySelector(".viewer").textContent.includes("5000 × 4000"))throw new Error("implicit large decode");
 await tap("viewer-close");await wait(()=>!document.querySelector(".viewer"));
 mark(10);await open("a-audio.wav");await wait(()=>!!document.querySelector(".viewer audio"));
 const lockedAudio=document.querySelector(".viewer audio");lastMedia=lockedAudio;await ready(lockedAudio);lockedAudio.muted=true;press(lockedAudio,".v-play");await wait(()=>lockedAudio.currentTime>0.2);
 document.querySelector("[data-qv-epoch]").dispatchEvent(new KeyboardEvent("keydown",{key:"l",ctrlKey:true,bubbles:true}));
 await wait(()=>!document.querySelector(".shell"));await wait(()=>reset(lockedAudio));
 // A new media request cannot reuse a capability after close/lock.
 mark(11);await refused(audioURL);
 passed=true;
}catch{
 const result=document.getElementById("validation-result");
 if(result&&lastMedia){result.value="MEDIA-FACTS:"+[Number(lastMedia.paused),Number(lastMedia.hasAttribute("src")),lastMedia.readyState,lastMedia.networkState,Number(!lastMedia.currentSrc),Number(lastMedia.isConnected)].join(",");result.dispatchEvent(new Event("input",{bubbles:true}));}
 /* Fixed numeric facts only; media URLs and platform messages stay out of logs. */
}
finally{
 const result=document.getElementById("validation-result");
 if(result){result.value=passed?"PASS":`FAIL:${stage}`;result.dispatchEvent(new Event("input",{bubbles:true}));}
}
})();
