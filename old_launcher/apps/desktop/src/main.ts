import './styles/tokens.css';
import { mount } from 'svelte';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('Failed to locate #app mount root');
}

export default mount(App, { target });
