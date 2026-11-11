import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { AppearanceProvider } from '../../src/appearance'
import { AppThemeButton } from '../../src/AppThemeButton'
import { PageManager } from '../../src/PageManager'
import { Screen } from '../../src/Screen'
import { display } from '../../src/layout'
import '../../src/styles.css'
function Fixture() {
 const [pages,setPages] = useState([{id:'main',name:'Codex 软件',layout:display.initialLayout}])
 const [rotation,setRotation] = useState(0)
 return <main className="studio" style={{padding:24,minHeight:'100vh'}}>
  <div className="studio-tools"><AppThemeButton /></div>
  <section style={{width:440,marginTop:24}}>
  <PageManager pages={pages} selected="main" disabled={false} onChange={setPages} onSelect={()=>{}}
    rotationSeconds={rotation} onRotationChange={setRotation} />
  <Screen surface="desktop" layout={pages[0].layout} snapshot={{revision:1,sequence:1,layout:pages[0].layout,values:{quota:68,speed:21,resets:1},lastSyncedAt:0,power:{source:'external'}}} /></section>
 </main>
}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><Fixture /></AppearanceProvider>)
