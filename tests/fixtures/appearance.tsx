import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { AppearanceProvider } from '../../src/appearance'
import { AppSettings } from '../../src/AppSettings'
import { PageManager } from '../../src/PageManager'
import { Screen } from '../../src/Screen'
import { display } from '../../src/layout'
import { Button } from '../../src/components/ui/button'
import '../../src/styles.css'
function Fixture() {
 const [open,setOpen] = useState(false)
 const [pages,setPages] = useState([{id:'main',name:'Codex 软件',layout:display.initialLayout}])
 const [rotation,setRotation] = useState(0)
 return <main className="studio" style={{padding:24,minHeight:'100vh'}}>
  <Button onClick={()=>setOpen(true)} className="self-start">设置</Button>
  <section style={{width:440,marginTop:24}}><PageManager pages={pages} selected="main" rotationSeconds={rotation} disabled={false} onChange={setPages} onRotationChange={setRotation} onSelect={()=>{}} />
  <Screen surface="desktop" layout={pages[0].layout} snapshot={{revision:1,sequence:1,layout:pages[0].layout,values:{quota:68,speed:21,resets:1},lastSyncedAt:0,power:{source:'external'}}} /></section>
  {open&&<AppSettings error="" onDismiss={()=>setOpen(false)} />}
 </main>
}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><Fixture /></AppearanceProvider>)
