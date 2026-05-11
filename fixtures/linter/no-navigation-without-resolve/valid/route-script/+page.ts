import { resolve } from '$app/paths';
import { goto } from '$app/navigation';

type Route = string;
const target: Route = resolve('/dashboard');

goto(target);
