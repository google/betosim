// Copyright 2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import{__decorate as t}from"../node_modules/tslib/tslib.es6.js";import{LitElement as e,html as r}from"https://cdn.jsdelivr.net/gh/lit/dist@2/core/lit-core.min.js";import{property as a,customElement as d}from"https://cdn.skypack.dev/pin/lit@v2.5.0-jYRq0AKQogjUdUh7SCAE/mode=imports/optimized/lit/decorators.js";import{simulationState as i}from"./device-observer.js";var s;let o=s=class extends e{constructor(){super(),this.action="move",this.addEventListener("dragstart",this.handleDragStart),this.addEventListener("dragend",this.handleDragEnd),this.addEventListener("click",this.handleSelect)}connectedCallback(){this.draggable=!0}handleDragStart(t){this.style.opacity="0.4",t.dataTransfer&&t.target&&(i.patchSelected(t.target.id),s.dragged=t.target,t.dataTransfer.effectAllowed="move"===this.action?"move":"copy")}handleDragEnd(){this.style.opacity="1",s.dragged=null}handleSelect(t){this.style.opacity="1",t.target&&i.patchSelected(t.target.id)}render(){return r` <slot></slot> `}};t([a({type:String,attribute:"action"})],o.prototype,"action",void 0),o=s=t([d("ns-device-dragzone")],o);export{o as DeviceDragZone};
