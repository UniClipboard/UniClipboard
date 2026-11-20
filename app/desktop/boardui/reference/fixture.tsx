import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {Dialog,Modal,ModalOverlay,Label} from 'react-aria-components';
import {RiSettings6Line,RiCloseLine} from '@remixicon/react';
import {Button} from '@/components/base/buttons/button';
import {Input} from '@/components/base/input/input';
import {Select,SelectItem} from '@/components/base/select/select';
import {DirectionProvider} from '@/components/foundations/direction/direction';
import './fixture.css';
const query=new URLSearchParams(location.search); document.documentElement.classList.toggle('dark',query.get('theme')==='dark');
const defaults={name:'Maya Chen',password:'secret',readonly:'Managed by workspace',disabled:'Not available',role:'member'};
function Form({close}:{close:()=>void}) {
 const [name,setName]=useState(defaults.name);const [password,setPassword]=useState(defaults.password);const [role,setRole]=useState(defaults.role);const [invalid,setInvalid]=useState(false);
 return <><header data-metric="dialog-header" className="flex flex-col gap-1"><h2 id="settings-title" className="text-title-2-medium text-text-primary">Workspace settings</h2><p className="text-body-regular text-text-secondary">Manage your profile and workspace access.</p></header><div data-metric="dialog-body" className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
 <Input data-testid="name" label="Name" hint={invalid?'Name is required.':'Your display name.'} value={name} onChange={v=>{setName(v);setInvalid(false)}} isRequired isInvalid={invalid} />
 <Input data-testid="password" label="Password" type="password" value={password} onChange={setPassword}/>
 <Input data-testid="readonly" label="Read-only" value={defaults.readonly} isReadOnly/>
 <Input data-testid="disabled" label="Disabled" value={defaults.disabled} isDisabled/>
 <div data-metric="role-field" className="flex flex-col gap-1"><label className="text-body-medium text-text-primary" id="role-label">Role</label><Select aria-labelledby="role-label" selectedKey={role} onSelectionChange={k=>setRole(String(k))} triggerClassName="w-full"><SelectItem id="member">Member</SelectItem><SelectItem id="admin">Administrator</SelectItem><SelectItem id="viewer">Viewer</SelectItem></Select></div>
 </div><footer data-metric="dialog-footer" className="flex justify-end gap-3"><Button variant="secondary" onClick={close}>Cancel</Button><Button onClick={()=>name.trim()?close():setInvalid(true)}>Save changes</Button></footer></>
}
function App(){const [open,setOpen]=useState(query.get('open')==='1'); return <DirectionProvider locale="en-US"><main className="flex min-h-full items-center justify-center bg-background-full"><Button onClick={()=>setOpen(true)}>Open settings</Button>
 {query.get('metrics')==='1'&&<div id="measurements" className="flex flex-col gap-6">{(['medium','small','xs'] as const).map(size=><div key={size} className="flex gap-4">{(['primary','secondary','ghost','danger'] as const).map(variant=><Button key={variant} size={size} variant={variant} data-metric={`button-${variant}-${size}`} leadingIcon={RiSettings6Line}>Continue</Button>)}</div>)}{(['medium','small'] as const).map(size=><Input key={size} size={size} label="Name" hint="Your display name." value="Maya Chen" data-testid={`measure-input-${size}`} />)}{(['md','sm'] as const).map(size=><Select key={size} size={size} data-testid={`measure-select-${size}`} aria-label={`Measure select ${size}`} defaultSelectedKey="member" className="w-60" triggerClassName="w-full"><SelectItem id="member">Member</SelectItem><SelectItem id="admin">Administrator</SelectItem></Select>)}</div>}
 <ModalOverlay isOpen={open} onOpenChange={setOpen} isDismissable className="fixed inset-0 z-100 flex items-center justify-center bg-black/70 p-4"><Modal className="flex max-h-[calc(100dvh-32px)] w-[480px] max-w-[calc(100vw-32px)] flex-col gap-6 rounded-3xl bg-background-full p-6 shadow-xs outline-none" data-metric="dialog"><Dialog aria-labelledby="settings-title" className="flex min-h-0 flex-col gap-6 outline-none"><Form close={()=>setOpen(false)}/></Dialog></Modal></ModalOverlay></main></DirectionProvider>}
createRoot(document.getElementById('root')!).render(<App/>);
