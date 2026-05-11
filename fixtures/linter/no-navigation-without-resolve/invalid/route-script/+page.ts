import { goto } from '$app/navigation';

type Route = `/${string}`;
const target: Route = '/dashboard';

goto(target);
