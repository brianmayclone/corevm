import axios from 'axios'

const api = axios.create({ baseURL: '' })

// Attach JWT to every request
api.interceptors.request.use((config) => {
  const token = localStorage.getItem('vmm_token')
  if (token) config.headers.Authorization = `Bearer ${token}`
  return config
})

// On 401 → redirect to login
api.interceptors.response.use(
  (res) => res,
  (err) => {
    if (err.response?.status === 401 && window.location.pathname !== '/login') {
      localStorage.removeItem('vmm_token')
      window.location.href = '/login'
    }
    return Promise.reject(err)
  },
)

export default api
