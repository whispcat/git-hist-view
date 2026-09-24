import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import './styles.css';
import { render } from 'solid-js/web';
import { App } from './ui/App';

render(() => <App />, document.getElementById('root')!);
