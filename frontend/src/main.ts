import { mount } from 'svelte'
import { setHashRoutingEnabled, setBasePath } from '@keenmate/svelte-spa-router/utils'
import './styles/global.css'
import App from './App.svelte'

setHashRoutingEnabled(false)
setBasePath('/')

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app
